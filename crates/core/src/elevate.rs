//! One UAC prompt per install: the unelevated process starts itself again with
//! "runas" and reads the worker's events from a named pipe it owns. The worker
//! never writes files on the caller's behalf; it only connects to the pipe.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::os::windows::io::FromRawHandle;

use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_INBOUND};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, INFINITE, WaitForSingleObject};
use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows::core::{HSTRING, PCWSTR};

/// Exit code when the user declines the UAC prompt (ERROR_CANCELLED).
pub const UAC_DECLINED: i32 = 1223;

#[derive(Debug, thiserror::Error)]
pub enum ElevateError {
    #[error("the administrator prompt was declined")]
    Declined,
    #[error("cannot start the elevated worker: {0}")]
    Launch(String),
    #[error("cannot create the progress pipe: {0}")]
    Pipe(String),
}

pub fn pipe_path(name: &str) -> String {
    format!(r"\\.\pipe\{name}")
}

/// Quotes one argument the way CommandLineToArgvW reads it back.
pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// Starts this exe elevated with `args` (which must include the pipe name the
/// worker connects to), hands every line it writes to `on_line`, and returns
/// the worker's exit code.
pub fn run_elevated(pipe_name: &str, args: &[String], mut on_line: impl FnMut(&str)) -> Result<i32, ElevateError> {
    let exe = std::env::current_exe().map_err(|e| ElevateError::Launch(e.to_string()))?;
    let path = HSTRING::from(pipe_path(pipe_name));
    // SAFETY: plain Win32 calls; the pipe handle is owned by `reader` below or closed on error.
    let pipe = unsafe {
        CreateNamedPipeW(
            &path,
            PIPE_ACCESS_INBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            0,
            64 * 1024,
            0,
            None,
        )
    };
    if pipe == INVALID_HANDLE_VALUE {
        return Err(ElevateError::Pipe(windows::core::Error::from_thread().to_string()));
    }

    let params = HSTRING::from(args.iter().map(|a| quote_arg(a)).collect::<Vec<_>>().join(" "));
    let file = HSTRING::from(exe.as_os_str());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: windows::core::w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: `info` points at live HSTRINGs for the duration of the call.
    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        unsafe {
            let _ = CloseHandle(pipe);
        }
        return Err(if e.code() == ERROR_CANCELLED.to_hresult() {
            ElevateError::Declined
        } else {
            ElevateError::Launch(e.to_string())
        });
    }
    let process = info.hProcess;

    // Read on a thread: if the worker dies before connecting, the main thread
    // connects to its own pipe once the process is gone, which unblocks it.
    let pipe_raw = pipe.0 as isize;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        let pipe = HANDLE(pipe_raw as *mut core::ffi::c_void);
        // SAFETY: valid pipe handle; ERROR_PIPE_CONNECTED means the client was faster.
        let connected = unsafe { ConnectNamedPipe(pipe, None) };
        if let Err(e) = connected
            && e.code() != ERROR_PIPE_CONNECTED.to_hresult() {
                unsafe {
                    let _ = CloseHandle(pipe);
                }
                return;
            }
        // SAFETY: ownership of the handle moves into the File, which closes it.
        let file = unsafe { File::from_raw_handle(pipe.0) };
        for line in BufReader::new(file).lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut exited = false;
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(line) => on_line(&line),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // SAFETY: process handle from ShellExecuteExW (NOCLOSEPROCESS).
                let done = unsafe { WaitForSingleObject(process, 0) }.0 == 0;
                if done && !exited {
                    exited = true;
                    // Unblock a reader still waiting in ConnectNamedPipe.
                    let _ = std::fs::OpenOptions::new().write(true).open(pipe_path(pipe_name));
                }
            }
        }
    }
    let _ = reader.join();

    let mut code = 0u32;
    // SAFETY: as above; the handle is closed once.
    unsafe {
        WaitForSingleObject(process, INFINITE);
        let _ = GetExitCodeProcess(process, &mut code);
        let _ = CloseHandle(process);
    }
    Ok(code as i32)
}

/// The worker side: the write end of the caller's pipe.
pub fn connect(pipe_name: &str) -> std::io::Result<File> {
    std::fs::OpenOptions::new().write(true).open(pipe_path(pipe_name))
}

#[cfg(test)]
mod tests {
    use super::quote_arg;

    #[test]
    fn quoting_follows_argv_rules() {
        assert_eq!(quote_arg("plain"), "plain");
        assert_eq!(quote_arg("with space"), "\"with space\"");
        assert_eq!(quote_arg(r"C:\dir with space\"), r#""C:\dir with space\\""#);
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_arg(""), "\"\"");
    }
}
