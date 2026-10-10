//! Small desktop facts the GUI needs: language, light/dark setting, work area,
//! and console attachment for the CLI (the exe is a GUI-subsystem program).

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};

/// True when the Windows display language is Turkish. This is the UI language,
/// not the regional format: an English Windows with Turkish dates stays English.
/// `CAIOR_LANG=tr|en` overrides it (handy for testing).
pub fn ui_language_is_turkish() -> bool {
    if let Ok(lang) = std::env::var("CAIOR_LANG") {
        return lang.eq_ignore_ascii_case("tr");
    }
    // SAFETY: no arguments, returns a LANGID.
    let langid = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    const LANG_TURKISH: u16 = 0x1f;
    langid & 0x3ff == LANG_TURKISH
}

/// Windows "app mode" setting. Missing value means dark is not requested.
pub fn apps_use_light_theme() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", KEY_READ)
        .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|v| v != 0)
        .unwrap_or(false)
}

/// Primary monitor area without the taskbar, in physical pixels: (left, top, right, bottom).
pub fn work_area() -> (i32, i32, i32, i32) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW};
    let mut r = RECT::default();
    // SAFETY: SPI_GETWORKAREA writes one RECT into the buffer.
    let ok = unsafe {
        SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut RECT as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    }
    .is_ok();
    if ok { (r.left, r.top, r.right, r.bottom) } else { (0, 0, 1920, 1040) }
}

/// For command-line use: attach to the console of the shell that started us,
/// so `println!` reaches it. Returns false when there is none (started from
/// Explorer), which is the GUI case.
pub fn attach_parent_console() -> bool {
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES,
    };
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
    use windows::core::w;
    // SAFETY: plain console API calls; the CONOUT$ handle stays open for the process lifetime.
    unsafe {
        // Output already redirected to a file or pipe: nothing to do.
        let redirected = GetStdHandle(STD_OUTPUT_HANDLE).is_ok_and(|h| !h.is_invalid() && !h.0.is_null());
        if redirected {
            return true;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            return false;
        }
        if let Ok(out) = CreateFileW(
            w!("CONOUT$"),
            (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        ) {
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, out);
            let _ = SetStdHandle(STD_ERROR_HANDLE, out);
        }
        true
    }
}
