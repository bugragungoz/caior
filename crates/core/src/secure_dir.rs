//! Folders that only administrators and SYSTEM can change. The elevated
//! session copies each verified installer into one before running it, so a
//! standard user can neither swap the file nor plant a DLL next to it (the
//! shared cache under %ProgramData% lets every user add files to subfolders).

use std::io;
use std::path::{Path, PathBuf};

/// SYSTEM and Administrators: full control. `P` blocks inherited entries, so
/// the parent folder's permissions (Users may add files) do not apply.
const ADMIN_ONLY: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
/// The same, plus read and list for Users (session logs they may open).
const ADMIN_WRITE_USERS_READ: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";

/// Who besides administrators may read the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readers {
    AdminsOnly,
    Users,
}

/// Creates `path` as a new folder with the given access. Fails when it exists
/// already: a folder someone else created first cannot be trusted.
pub fn create(path: &Path, readers: Readers) -> io::Result<()> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::CreateDirectoryW;
    use windows::core::HSTRING;

    let sddl = HSTRING::from(match readers {
        Readers::AdminsOnly => ADMIN_ONLY,
        Readers::Users => ADMIN_WRITE_USERS_READ,
    });
    let target = HSTRING::from(path.as_os_str());
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sd` receives a LocalAlloc'd descriptor that is freed below;
    // `sa` and both strings outlive the CreateDirectoryW call.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None)?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let created = CreateDirectoryW(&target, Some(&sa));
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        created?;
    }
    Ok(())
}

/// Creates `<parent>\<prefix><random>` with `create`, trying new names while
/// one is taken.
pub fn create_unique(parent: &Path, prefix: &str, readers: Readers) -> io::Result<PathBuf> {
    for _ in 0..16 {
        let path = parent.join(format!("{prefix}{:016x}", random()));
        match create(&path, readers) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free folder name"))
}

/// Unpredictable enough for a folder name; uniqueness is enforced by `create`.
fn random() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.write_u32(std::process::id());
    h.finish()
}

/// `%SystemRoot%\Temp`: Users may create folders there but cannot delete or
/// rename other people's, unlike %TEMP% or %ProgramData%.
pub fn system_temp() -> PathBuf {
    PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join("Temp")
}
