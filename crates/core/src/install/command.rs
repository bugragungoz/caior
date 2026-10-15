//! Command lines per install kind. Pure functions, so every kind is unit tested
//! without running anything.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::catalog::{InstallKind, Package};

/// Where a session keeps its logs and scratch files, and where Windows lives.
#[derive(Debug, Clone)]
pub struct RunContext {
    pub log_dir: PathBuf,
    /// Scratch space, e.g. for the extracted DirectX redist.
    pub work_dir: PathBuf,
    /// `%SystemRoot%`, usually `C:\Windows`.
    pub system_root: PathBuf,
}

impl RunContext {
    fn system32(&self, exe: &str) -> PathBuf {
        self.system_root.join("System32").join(exe)
    }
}

/// One process to start. `args` is passed to Windows verbatim, so vendor
/// switches keep their exact quoting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: String,
    /// File that receives the process's stdout and stderr.
    pub output_log: PathBuf,
    /// Extra log written by the tool itself (msiexec /l*v, DISM /LogPath).
    pub tool_log: Option<PathBuf>,
}

fn quote(p: &Path) -> String {
    format!("\"{}\"", p.display())
}

/// Base64 of UTF-16LE, the format of `powershell -EncodedCommand`.
fn encode_powershell(script: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// PowerShell single-quoted literal.
fn ps_literal(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "''"))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("package {0} needs a downloaded installer")]
    NoInstaller(String),
    #[error("package {id}: install.{field} is missing")]
    Missing { id: String, field: &'static str },
}

/// The processes that install `pkg`, in order. `installer` is the verified file
/// for kinds that have one.
pub fn invocations(pkg: &Package, installer: Option<&Path>, ctx: &RunContext) -> Result<Vec<Invocation>, CommandError> {
    let id = &pkg.id;
    let out_log = |suffix: &str| ctx.log_dir.join(format!("{id}{suffix}.log"));
    let need_file = || installer.ok_or_else(|| CommandError::NoInstaller(id.clone()));
    let i = &pkg.install;
    let inv = match i.kind {
        InstallKind::Exe => {
            // `{log}` in the catalog args becomes a per-package log file, for
            // installers that take a log path (Burn's /log, msiexec /l*v inside IExpress).
            let args = i.args.clone().unwrap_or_default();
            let installer_log = out_log(".installer");
            let tool_log = args.contains("{log}").then(|| installer_log.clone());
            vec![Invocation {
                program: need_file()?.to_path_buf(),
                args: args.replace("{log}", &installer_log.display().to_string()),
                output_log: out_log(""),
                tool_log,
            }]
        }
        InstallKind::Msi => {
            let msi_log = out_log(".msi");
            let mut args = format!("/i {} /qn /norestart /l*v {}", quote(need_file()?), quote(&msi_log));
            if let Some(p) = i.properties.as_deref().filter(|p| !p.trim().is_empty()) {
                args.push(' ');
                args.push_str(p.trim());
            }
            vec![Invocation { program: ctx.system32("msiexec.exe"), args, output_log: out_log(""), tool_log: Some(msi_log) }]
        }
        InstallKind::Msu => vec![Invocation {
            program: ctx.system32("wusa.exe"),
            args: format!("{} /quiet /norestart", quote(need_file()?)),
            output_log: out_log(""),
            tool_log: None,
        }],
        InstallKind::Appx => {
            let file = ps_literal(need_file()?);
            let action = if i.provisioned {
                format!("Add-AppxProvisionedPackage -Online -PackagePath {file} -SkipLicense | Out-Null")
            } else {
                format!("Add-AppxPackage -Path {file}")
            };
            // 0x80073D06 (ERROR_INSTALL_PACKAGE_DOWNGRADE): a newer version of the
            // package is already installed, e.g. the framework that came with the
            // full Windows App Runtime installer earlier in the same run. The
            // package is present, so that counts as success.
            let script = format!(
                "$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue'; \
                 try {{ {action}; exit 0 }} catch {{ \
                 if ($_.Exception.HResult -eq -2147009274 -or \"$_\" -match '0x80073D06') {{ [Console]::Out.WriteLine('A newer version is already installed.'); exit 0 }}; \
                 [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}"
            );
            vec![Invocation {
                program: ctx.system32(r"WindowsPowerShell\v1.0\powershell.exe"),
                args: format!("-NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand {}", encode_powershell(&script)),
                output_log: out_log(""),
                tool_log: None,
            }]
        }
        InstallKind::DismFeature => {
            let feature = i.feature.as_deref().ok_or(CommandError::Missing { id: id.clone(), field: "feature" })?;
            let dism_log = out_log(".dism");
            let all = if i.all.unwrap_or(false) { " /All" } else { "" };
            vec![Invocation {
                program: ctx.system32("dism.exe"),
                args: format!(
                    "/Online /Enable-Feature /FeatureName:{feature}{all} /NoRestart /Quiet /LogPath:{}",
                    quote(&dism_log)
                ),
                output_log: out_log(""),
                tool_log: Some(dism_log),
            }]
        }
        InstallKind::DismCapability => {
            let cap = i.capability.as_deref().ok_or(CommandError::Missing { id: id.clone(), field: "capability" })?;
            let dism_log = out_log(".dism");
            vec![Invocation {
                program: ctx.system32("dism.exe"),
                args: format!("/Online /Add-Capability /CapabilityName:{cap} /NoRestart /Quiet /LogPath:{}", quote(&dism_log)),
                output_log: out_log(""),
                tool_log: Some(dism_log),
            }]
        }
        InstallKind::Directx => {
            let dir = ctx.work_dir.join(format!("{id}-redist"));
            vec![
                Invocation {
                    program: need_file()?.to_path_buf(),
                    args: format!("/Q /T:{}", quote(&dir)),
                    output_log: out_log(".extract"),
                    tool_log: None,
                },
                Invocation {
                    program: dir.join("DXSETUP.exe"),
                    args: "/silent".into(),
                    output_log: out_log(""),
                    tool_log: None,
                },
            ]
        }
    };
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::parse_category;

    fn pkg(install: &str, source: bool) -> Package {
        let src = if source {
            "[package.source]\nurl = \"https://example.com/f\"\nfilename = \"f.exe\"\n"
        } else {
            ""
        };
        let text = format!(
            "[category]\nid = \"t\"\nname = {{ en = \"T\" }}\norder = 1\nicon = \"x\"\n\n[[package]]\nid = \"p\"\nname = \"P\"\nversion = \"1\"\narch = \"x64\"\norder = 1\n{src}[package.install]\n{install}\n[package.detect]\nkind = \"none\"\n"
        );
        parse_category("t.toml", &text).unwrap().packages.remove(0)
    }

    fn ctx() -> RunContext {
        RunContext {
            log_dir: PathBuf::from(r"C:\logs"),
            work_dir: PathBuf::from(r"C:\work"),
            system_root: PathBuf::from(r"C:\Windows"),
        }
    }

    #[test]
    fn exe_args_are_verbatim() {
        let p = pkg("kind = \"exe\"\nargs = 'INSTALLDIR=\"C:\\x y\" /quiet'", true);
        let inv = invocations(&p, Some(Path::new(r"C:\cache\p\f.exe")), &ctx()).unwrap();
        assert_eq!(inv[0].program, PathBuf::from(r"C:\cache\p\f.exe"));
        assert_eq!(inv[0].args, r#"INSTALLDIR="C:\x y" /quiet"#);
    }

    #[test]
    fn log_placeholder_in_exe_args() {
        let p = pkg("kind = \"exe\"
args = '/quiet /log \"{log}\"'", true);
        let inv = invocations(&p, Some(Path::new(r"C:\cache\p\f.exe")), &ctx()).unwrap();
        let log = ctx().log_dir.join("p.installer.log");
        assert_eq!(inv[0].args, format!(r#"/quiet /log "{}""#, log.display()));
        assert_eq!(inv[0].tool_log, Some(log));
    }

    #[test]
    fn msi_gets_quiet_flags_log_and_properties() {
        let p = pkg("kind = \"msi\"\nproperties = \"ADDLOCAL=FeatureMain\"", true);
        let inv = invocations(&p, Some(Path::new(r"C:\cache\p\f.msi")), &ctx()).unwrap();
        assert_eq!(inv[0].program, ctx().system_root.join("System32").join("msiexec.exe"));
        let log = ctx().log_dir.join("p.msi.log");
        assert_eq!(inv[0].args, format!(r#"/i "C:\cache\p\f.msi" /qn /norestart /l*v "{}" ADDLOCAL=FeatureMain"#, log.display()));
    }

    #[test]
    fn dism_feature_and_capability() {
        let f = pkg("kind = \"dism-feature\"\nfeature = \"NetFx3\"\nall = true", false);
        let inv = invocations(&f, None, &ctx()).unwrap();
        assert_eq!(
            inv[0].args,
            format!(
                r#"/Online /Enable-Feature /FeatureName:NetFx3 /All /NoRestart /Quiet /LogPath:"{}""#,
                ctx().log_dir.join("p.dism.log").display()
            )
        );
        let c = pkg("kind = \"dism-capability\"\ncapability = \"Media.MediaFeaturePack~~~~0.0.1.0\"", false);
        assert!(invocations(&c, None, &ctx()).unwrap()[0].args.contains("/Add-Capability /CapabilityName:Media.MediaFeaturePack~~~~0.0.1.0"));
    }

    #[test]
    fn directx_extracts_then_runs_dxsetup() {
        let p = pkg("kind = \"directx\"", true);
        let inv = invocations(&p, Some(Path::new(r"C:\cache\p\dx.exe")), &ctx()).unwrap();
        assert_eq!(inv.len(), 2);
        let dir = ctx().work_dir.join("p-redist");
        assert_eq!(inv[0].args, format!(r#"/Q /T:"{}""#, dir.display()));
        assert_eq!(inv[1].program, dir.join("DXSETUP.exe"));
        assert_eq!(inv[1].args, "/silent");
    }

    #[test]
    fn missing_installer_is_an_error() {
        let p = pkg("kind = \"exe\"\nargs = \"\"", true);
        assert_eq!(invocations(&p, None, &ctx()).unwrap_err(), CommandError::NoInstaller("p".into()));
    }

    #[test]
    fn powershell_encoding_matches_reference() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("exit 0"))
        assert_eq!(encode_powershell("exit 0"), "ZQB4AGkAdAAgADAA");
        assert_eq!(encode_powershell("ab"), "YQBiAA==");
        assert_eq!(ps_literal(Path::new(r"C:\it's\a.msix")), r"'C:\it''s\a.msix'");
    }
}
