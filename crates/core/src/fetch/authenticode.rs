//! Authenticode check through WinVerifyTrust. Works offline: revocation is not
//! checked and no URL retrieval happens, so offline bundles verify the same way.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signature {
    /// True when WinVerifyTrust accepted the signature and its chain.
    pub valid: bool,
    /// WinVerifyTrust result (0 = success), useful in logs.
    pub status: i32,
    /// Simple display name (usually the CN) of the signing certificate.
    pub signer: Option<String>,
}

#[cfg(windows)]
pub fn check(path: &Path) -> Signature {
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Security::Cryptography::{CERT_NAME_SIMPLE_DISPLAY_TYPE, CertGetNameStringW};
    use windows::Win32::Security::WinTrust::{
        WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
        WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
        WTD_STATEACTION_VERIFY, WTD_UI_NONE, WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData,
        WinVerifyTrust,
    };
    use windows::core::PCWSTR;

    let wide: Vec<u16> = path.as_os_str().encode_wide_nul();
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: PCWSTR(wide.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;

    // SAFETY: `file` and `wide` outlive both WinVerifyTrust calls; the state is
    // closed with WTD_STATEACTION_CLOSE before returning; the signer pointers
    // come from that state and are only read while it is open.
    unsafe {
        let status = WinVerifyTrust(HWND::default(), &mut action, (&mut data as *mut WINTRUST_DATA).cast());
        let mut signer = None;
        let provider = WTHelperProvDataFromStateData(data.hWVTStateData);
        if !provider.is_null() {
            let sgnr = WTHelperGetProvSignerFromChain(provider, 0, false, 0);
            if !sgnr.is_null() && (*sgnr).csCertChain > 0 && !(*sgnr).pasCertChain.is_null() {
                let cert = (*(*sgnr).pasCertChain).pCert;
                if !cert.is_null() {
                    let mut buf = [0u16; 256];
                    let n = CertGetNameStringW(cert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut buf));
                    if n > 1 {
                        signer = Some(String::from_utf16_lossy(&buf[..(n - 1) as usize]));
                    }
                }
            }
        }
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(HWND::default(), &mut action, (&mut data as *mut WINTRUST_DATA).cast());
        Signature { valid: status == 0, status, signer }
    }
}

#[cfg(not(windows))]
pub fn check(_path: &Path) -> Signature {
    Signature { valid: false, status: -1, signer: None }
}

trait EncodeWideNul {
    fn encode_wide_nul(&self) -> Vec<u16>;
}

#[cfg(windows)]
impl EncodeWideNul for std::ffi::OsStr {
    fn encode_wide_nul(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(std::iter::once(0)).collect()
    }
}
