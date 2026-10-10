//! The running Windows: build, native architecture, edition. Decides which
//! packages apply to this machine at all.

use serde::Serialize;

use crate::catalog::{Arch, Package};
use crate::version::Version;

/// First Windows 11 build; x64 emulation on ARM64 starts here.
const WIN11_BUILD: u64 = 22000;

#[derive(Debug, Clone, Serialize)]
pub struct SystemInfo {
    /// 10.0.<build>.<ubr>
    pub os_version: Version,
    /// Native architecture, also when this process runs emulated.
    pub os_arch: Arch,
    /// EditionID, e.g. "Professional", "CoreN".
    pub edition_id: String,
    /// ProductName as stored in the registry (Windows 11 still says "Windows 10" there).
    pub product_name: String,
    /// DisplayVersion, e.g. "23H2".
    pub display_version: String,
    pub elevated: bool,
    /// Windows is waiting for a restart (servicing, Windows Update or pending
    /// file renames). Some installers, .NET Framework among them, fail until then.
    #[serde(default)]
    pub reboot_pending: bool,
}

impl SystemInfo {
    pub fn build(&self) -> u64 {
        self.os_version.parts().get(2).copied().unwrap_or(0)
    }

    pub fn is_windows_11(&self) -> bool {
        self.build() >= WIN11_BUILD
    }

    /// N editions ship without media components: "CoreN", "ProfessionalN", "EnterpriseN", ...
    pub fn is_n_edition(&self) -> bool {
        self.edition_id.ends_with('N')
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
pub enum NotApplicable {
    OsTooOld { min: String },
    OsTooNew { max: String },
    Arch { os: Arch, package: Arch },
    NotNEdition,
}

/// Default OS architectures for a package architecture. x86 code runs everywhere,
/// x64 on x64 and on Windows 11 ARM64, arm64 only natively.
fn default_os_arch(pkg: Arch) -> &'static [Arch] {
    match pkg {
        Arch::X86 => &[Arch::X86, Arch::X64, Arch::Arm64],
        Arch::X64 => &[Arch::X64, Arch::Arm64],
        Arch::Arm64 => &[Arch::Arm64],
        Arch::Neutral => &[Arch::X86, Arch::X64, Arch::Arm64],
    }
}

pub fn applicability(pkg: &Package, sys: &SystemInfo) -> Result<(), NotApplicable> {
    if let Some(min) = pkg.os_min.as_deref().and_then(|m| m.parse::<Version>().ok()) {
        if sys.os_version < min {
            return Err(NotApplicable::OsTooOld { min: min.to_string() });
        }
    }
    if let Some(max) = pkg.os_max.as_deref().and_then(|m| m.parse::<Version>().ok()) {
        if sys.os_version.truncated(max.parts().len()) > max {
            return Err(NotApplicable::OsTooNew { max: max.to_string() });
        }
    }
    let allowed: &[Arch] = if pkg.os_arch.is_empty() { default_os_arch(pkg.arch) } else { &pkg.os_arch };
    let arch_ok = allowed.contains(&sys.os_arch)
        && !(pkg.arch == Arch::X64 && sys.os_arch == Arch::Arm64 && !sys.is_windows_11());
    if !arch_ok {
        return Err(NotApplicable::Arch { os: sys.os_arch, package: pkg.arch });
    }
    if pkg.only_n_edition && !sys.is_n_edition() {
        return Err(NotApplicable::NotNEdition);
    }
    Ok(())
}
