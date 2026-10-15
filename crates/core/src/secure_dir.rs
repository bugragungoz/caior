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
        ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut sd, None).map_err(io_error)?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let created = CreateDirectoryW(&target, Some(&sa));
        let _ = LocalFree(Some(HLOCAL(sd.0)));
        created.map_err(io_error)?;
    }
    Ok(())
}

/// The plain Win32 error, so `kind()` works: the windows crate's own
/// conversion keeps the HRESULT (0x800700B7), which reads as Uncategorized
/// instead of AlreadyExists.
fn io_error(e: windows::core::Error) -> io::Error {
    let hr = e.code().0 as u32;
    if hr & 0xffff_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((hr & 0xffff) as i32)
    } else {
        io::Error::other(e)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `icacls` prints account names in the system language, but the flags
    /// are the same everywhere: `(I)` marks an inherited entry.
    fn acl(path: &Path) -> String {
        let out = std::process::Command::new("icacls").arg(path).output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[test]
    fn private_folder_has_only_its_own_entries() {
        let base = std::env::temp_dir().join(format!("caior-secure-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let admins = base.join("admins");
        create(&admins, Readers::AdminsOnly).unwrap();
        let text = acl(&admins);
        assert!(!text.contains("(I)"), "nothing inherited:\n{text}");
        assert_eq!(text.matches("(OI)(CI)(F)").count(), 2, "SYSTEM and Administrators only:\n{text}");
        assert!(!text.contains("(RX)"), "{text}");
        assert_eq!(create(&admins, Readers::AdminsOnly).unwrap_err().kind(), io::ErrorKind::AlreadyExists);

        let logs = create_unique(&base, "logs-", Readers::Users).unwrap();
        let text = acl(&logs);
        assert!(!text.contains("(I)"), "{text}");
        assert_eq!(text.matches("(OI)(CI)(RX)").count(), 1, "Users may read:\n{text}");
        assert_ne!(create_unique(&base, "logs-", Readers::Users).unwrap(), logs);
        let _ = std::fs::remove_dir_all(&base);
    }
}
