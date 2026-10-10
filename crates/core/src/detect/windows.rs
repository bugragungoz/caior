//! Windows implementation of `Probe` and `SystemInfo`. Everything here works
//! without administrator rights except capability queries.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::ApplicationInstallationAndServicing::{
    INSTALLSTATE_DEFAULT, MsiEnumRelatedProductsW, MsiGetProductInfoW, MsiQueryProductStateW,
};
use windows::Win32::System::SystemInformation::{
    IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64, IMAGE_FILE_MACHINE_I386,
};
use windows::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};

use super::probe::{Found, Probe, RegData, UninstallEntry, Unavailable};
use super::system::SystemInfo;
use crate::catalog::{Arch, Hive, RegView};
use crate::version::Version;

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

fn open(hive: Hive, view: RegView, key: &str) -> Option<RegKey> {
    let root = RegKey::predef(match hive {
        Hive::HKLM => HKEY_LOCAL_MACHINE,
        Hive::HKCU => HKEY_CURRENT_USER,
    });
    let flag = match view {
        RegView::Bits32 => KEY_WOW64_32KEY,
        RegView::Bits64 | RegView::Both => KEY_WOW64_64KEY,
    };
    root.open_subkey_with_flags(key, KEY_READ | flag).ok()
}

fn read_value(key: &RegKey, name: &str) -> Option<RegData> {
    if let Ok(v) = key.get_value::<u32, _>(name) {
        return Some(RegData::Dword(v));
    }
    if let Ok(v) = key.get_value::<u64, _>(name) {
        return Some(RegData::Qword(v));
    }
    if let Ok(v) = key.get_value::<String, _>(name) {
        return Some(RegData::Str(v));
    }
    key.get_raw_value(name).ok().map(|_| RegData::Other)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Expands `%NAME%` from the environment; unknown names stay as they are.
fn expand_env(path: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"%([^%]+)%").unwrap());
    re.replace_all(path, |c: &regex::Captures| std::env::var(&c[1]).unwrap_or_else(|_| c[0].to_string()))
        .into_owned()
}

fn msi_version(product_code: &str) -> Option<Version> {
    let code = wide(product_code);
    let mut buf = vec![0u16; 64];
    let mut len = buf.len() as u32;
    // SAFETY: buffers are valid for the given lengths and NUL terminated.
    let rc = unsafe {
        MsiGetProductInfoW(PCWSTR(code.as_ptr()), w!("VersionString"), Some(PWSTR(buf.as_mut_ptr())), Some(&mut len))
    };
    if rc != 0 {
        return None;
    }
    Version::parse_lenient(&String::from_utf16_lossy(&buf[..len as usize]))
}

fn file_version_of(path: &str) -> Option<Version> {
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };
    let p = wide(path);
    // SAFETY: standard version-info sequence; the buffer outlives every pointer into it.
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(p.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(p.as_ptr()), None, size, data.as_mut_ptr().cast()).ok()?;
        let mut info: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(data.as_ptr().cast(), w!("\\"), &mut info, &mut len).as_bool()
            || info.is_null()
            || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
        {
            return None;
        }
        let fixed = &*(info as *const VS_FIXEDFILEINFO);
        let (ms, ls) = (fixed.dwFileVersionMS, fixed.dwFileVersionLS);
        format!("{}.{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16, ls & 0xffff).parse().ok()
    }
}

#[derive(serde::Deserialize)]
#[allow(non_snake_case)]
struct OptionalFeature {
    InstallState: u32,
}

/// Reads the live system. Results of expensive enumerations are cached for the
/// lifetime of the probe, so create a new one to re-scan after installing.
#[derive(Default)]
pub struct WindowsProbe {
    uninstall: OnceCell<HashMap<&'static str, Vec<UninstallEntry>>>,
    wmi: OnceCell<Option<wmi::WMIConnection>>,
}

impl WindowsProbe {
    pub fn new() -> Self {
        Self::default()
    }

    fn uninstall_cache(&self) -> &HashMap<&'static str, Vec<UninstallEntry>> {
        self.uninstall.get_or_init(|| {
            let read = |hive: Hive, view: RegView| -> Vec<UninstallEntry> {
                let Some(root) = open(hive, view, UNINSTALL) else { return Vec::new() };
                root.enum_keys()
                    .filter_map(Result::ok)
                    .filter_map(|name| root.open_subkey_with_flags(&name, KEY_READ).ok())
                    .filter_map(|k| {
                        let display_name: String = k.get_value("DisplayName").ok()?;
                        Some(UninstallEntry {
                            display_name,
                            display_version: k.get_value("DisplayVersion").ok(),
                            publisher: k.get_value("Publisher").ok(),
                        })
                    })
                    .collect()
            };
            HashMap::from([
                ("hklm64", read(Hive::HKLM, RegView::Bits64)),
                ("hklm32", read(Hive::HKLM, RegView::Bits32)),
                ("hkcu", read(Hive::HKCU, RegView::Bits64)),
            ])
        })
    }

    fn wmi(&self) -> Option<&wmi::WMIConnection> {
        self.wmi.get_or_init(|| wmi::WMIConnection::new().ok()).as_ref()
    }
}

impl Probe for WindowsProbe {
    fn reg_key_exists(&self, hive: Hive, view: RegView, key: &str) -> bool {
        open(hive, view, key).is_some()
    }

    fn reg_value(&self, hive: Hive, view: RegView, key: &str, name: &str) -> Option<RegData> {
        read_value(&open(hive, view, key)?, name)
    }

    fn reg_names(&self, hive: Hive, view: RegView, key: &str) -> Vec<String> {
        let Some(k) = open(hive, view, key) else { return Vec::new() };
        let values = k.enum_values().filter_map(Result::ok).map(|(name, _)| name);
        let keys = k.enum_keys().filter_map(Result::ok);
        values.chain(keys).collect()
    }

    fn uninstall_entries(&self, view: RegView) -> Vec<UninstallEntry> {
        let cache = self.uninstall_cache();
        let parts: &[&str] = match view {
            RegView::Bits64 => &["hklm64", "hkcu"],
            RegView::Bits32 => &["hklm32", "hkcu"],
            RegView::Both => &["hklm64", "hklm32", "hkcu"],
        };
        parts.iter().flat_map(|p| cache[p].iter().cloned()).collect()
    }

    fn msi_product(&self, product_code: &str) -> Option<Found> {
        let code = wide(product_code);
        // SAFETY: NUL-terminated wide string.
        let state = unsafe { MsiQueryProductStateW(PCWSTR(code.as_ptr())) };
        (state == INSTALLSTATE_DEFAULT).then(|| Found { version: msi_version(product_code) })
    }

    fn msi_related(&self, upgrade_code: &str) -> Vec<Found> {
        let code = wide(upgrade_code);
        let mut out = Vec::new();
        for index in 0.. {
            let mut buf = [0u16; 39];
            // SAFETY: the product buffer must hold 39 characters (GUID + NUL).
            let rc = unsafe { MsiEnumRelatedProductsW(PCWSTR(code.as_ptr()), None, index, PWSTR(buf.as_mut_ptr())) };
            if rc != 0 {
                break;
            }
            let product = String::from_utf16_lossy(&buf[..38]);
            if let Some(f) = self.msi_product(&product) {
                out.push(f);
            }
        }
        out
    }

    fn file_version(&self, path: &str) -> Option<Found> {
        let expanded = expand_env(path);
        std::path::Path::new(&expanded)
            .is_file()
            .then(|| Found { version: file_version_of(&expanded) })
    }

    fn optional_feature_enabled(&self, name: &str) -> Result<bool, Unavailable> {
        if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
            return Err(Unavailable(format!("invalid feature name {name:?}")));
        }
        let con = self.wmi().ok_or_else(|| Unavailable("WMI is not available".into()))?;
        let rows: Vec<OptionalFeature> = con
            .raw_query(format!("SELECT InstallState FROM Win32_OptionalFeature WHERE Name = '{name}'"))
            .map_err(|e| Unavailable(format!("WMI query failed: {e}")))?;
        // InstallState: 1 enabled, 2 disabled, 3 absent, 4 unknown.
        Ok(rows.iter().any(|r| r.InstallState == 1))
    }

    fn capability_installed(&self, _name: &str) -> Result<bool, Unavailable> {
        // Capability state needs the DISM API with administrator rights; the
        // elevated worker re-checks before installing.
        Err(Unavailable("capability state needs administrator rights".into()))
    }

    fn appx_versions(&self, family: &str) -> Result<Vec<Version>, Unavailable> {
        use windows::Management::Deployment::PackageManager;
        let manager = PackageManager::new().map_err(|e| Unavailable(format!("PackageManager: {e}")))?;
        let packages = manager
            .FindPackagesByUserSecurityIdPackageFamilyName(&HSTRING::new(), &HSTRING::from(family))
            .map_err(|e| Unavailable(format!("appx query failed: {e}")))?;
        let mut versions = Vec::new();
        for p in packages {
            if let Ok(v) = p.Id().and_then(|id| id.Version())
                && let Ok(v) = format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision).parse() {
                    versions.push(v);
                }
        }
        Ok(versions)
    }
}

fn machine_to_arch(m: IMAGE_FILE_MACHINE) -> Option<Arch> {
    match m {
        IMAGE_FILE_MACHINE_AMD64 => Some(Arch::X64),
        IMAGE_FILE_MACHINE_ARM64 => Some(Arch::Arm64),
        IMAGE_FILE_MACHINE_I386 => Some(Arch::X86),
        _ => None,
    }
}

fn native_arch() -> Arch {
    let mut process = IMAGE_FILE_MACHINE::default();
    let mut native = IMAGE_FILE_MACHINE::default();
    // SAFETY: pseudo handle of the current process; out pointers are valid.
    let ok = unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, Some(&mut native)) }.is_ok();
    let compiled = if cfg!(target_arch = "aarch64") {
        Arch::Arm64
    } else if cfg!(target_arch = "x86") {
        Arch::X86
    } else {
        Arch::X64
    };
    if ok { machine_to_arch(native).unwrap_or(compiled) } else { compiled }
}

fn is_elevated() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows::Win32::System::Threading::OpenProcessToken;
    // SAFETY: token handle is closed below; the out buffer is a TOKEN_ELEVATION.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

fn reboot_pending() -> bool {
    let key = |path: &str| open(Hive::HKLM, RegView::Bits64, path).is_some();
    key(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending")
        || key(r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired")
        || open(Hive::HKLM, RegView::Bits64, r"SYSTEM\CurrentControlSet\Control\Session Manager")
            .and_then(|k| k.get_value::<Vec<String>, _>("PendingFileRenameOperations").ok())
            .is_some_and(|v| v.iter().any(|s| !s.is_empty()))
}

impl SystemInfo {
    pub fn current() -> SystemInfo {
        let cv = open(Hive::HKLM, RegView::Bits64, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
        let get_str = |name: &str| cv.as_ref().and_then(|k| k.get_value::<String, _>(name).ok()).unwrap_or_default();
        let get_u32 = |name: &str| cv.as_ref().and_then(|k| k.get_value::<u32, _>(name).ok());
        let major = get_u32("CurrentMajorVersionNumber").unwrap_or(10);
        let minor = get_u32("CurrentMinorVersionNumber").unwrap_or(0);
        let build = get_str("CurrentBuildNumber");
        let ubr = get_u32("UBR").unwrap_or(0);
        let os_version = format!("{major}.{minor}.{}.{ubr}", if build.is_empty() { "0" } else { &build })
            .parse()
            .unwrap_or_default();
        SystemInfo {
            os_version,
            os_arch: native_arch(),
            edition_id: get_str("EditionID"),
            product_name: get_str("ProductName"),
            display_version: get_str("DisplayVersion"),
            elevated: is_elevated(),
            reboot_pending: reboot_pending(),
        }
    }
}
