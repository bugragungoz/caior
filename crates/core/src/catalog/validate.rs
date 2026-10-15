//! Catalog validation. Rules are listed in `docs/CATALOG-FORMAT.md`.

use std::collections::{HashMap, HashSet};
use std::fmt;

use regex::Regex;

use super::model::*;
use crate::version::Version;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub severity: Severity,
    pub file: String,
    pub package: Option<String>,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        match &self.package {
            Some(p) => write!(f, "{level}: {} [{p}]: {}", self.file, self.message),
            None => write!(f, "{level}: {}: {}", self.file, self.message),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ValidateOptions {
    /// Require hashes and sizes (release builds). Drafts may leave them empty.
    pub strict: bool,
}

struct Ctx<'a> {
    issues: Vec<Issue>,
    file: &'a str,
    package: Option<&'a str>,
}

impl Ctx<'_> {
    fn push(&mut self, severity: Severity, message: impl Into<String>) {
        self.issues.push(Issue {
            severity,
            file: self.file.to_string(),
            package: self.package.map(str::to_string),
            message: message.into(),
        });
    }
    fn error(&mut self, message: impl Into<String>) {
        self.push(Severity::Error, message);
    }
    fn warn(&mut self, message: impl Into<String>) {
        self.push(Severity::Warning, message);
    }
}

pub fn validate(catalog: &Catalog, opts: ValidateOptions) -> Vec<Issue> {
    let id_re = Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap();
    let guid_re = Regex::new(r"^\{[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}\}$").unwrap();
    let hex_re = Regex::new(r"^[0-9a-f]{64}$").unwrap();

    let mut issues = Vec::new();
    let mut category_ids = HashSet::new();
    let mut package_file: HashMap<&str, &str> = HashMap::new();
    let mut group_category: HashMap<&str, &str> = HashMap::new();

    for entry in &catalog.categories {
        let mut ctx = Ctx { issues: Vec::new(), file: &entry.file, package: None };
        let cat = &entry.category;
        if !id_re.is_match(&cat.id) {
            ctx.error(format!("category id {:?} must match [a-z0-9][a-z0-9-]*", cat.id));
        }
        if !category_ids.insert(cat.id.as_str()) {
            ctx.error(format!("duplicate category id {:?}", cat.id));
        }
        check_text(&mut ctx, "category.name", &cat.name);
        if let Some(d) = &cat.description {
            check_text(&mut ctx, "category.description", d);
        }
        if entry.packages.is_empty() {
            ctx.warn("category has no packages");
        }

        for pkg in &entry.packages {
            ctx.package = Some(&pkg.id);
            if !id_re.is_match(&pkg.id) {
                ctx.error("id must match [a-z0-9][a-z0-9-]*");
            }
            if let Some(prev) = package_file.insert(&pkg.id, &entry.file) {
                ctx.error(format!("duplicate package id (also in {prev})"));
            }
            if let Some(group) = &pkg.group
                && let Some(prev) = group_category.insert(group, &cat.id)
                    && prev != cat.id {
                        ctx.error(format!("group {group:?} spans categories {prev} and {}", cat.id));
                    }
            check_package(&mut ctx, pkg, opts, &guid_re, &hex_re);
        }
        issues.append(&mut ctx.issues);
    }

    check_dependencies(catalog, &mut issues);
    issues.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.file.cmp(&b.file)));
    issues
}

fn check_text(ctx: &mut Ctx, field: &str, text: &Text) {
    if text.en.trim().is_empty() {
        ctx.error(format!("{field}.en is empty"));
    }
    if text.tr.as_deref().is_none_or(|t| t.trim().is_empty()) {
        ctx.warn(format!("{field}.tr is missing"));
    }
}

fn check_version(ctx: &mut Ctx, field: &str, value: &Option<String>) {
    if let Some(v) = value
        && v.parse::<Version>().is_err() {
            ctx.error(format!("{field} {v:?} is not a dotted numeric version"));
        }
}

fn check_package(ctx: &mut Ctx, pkg: &Package, opts: ValidateOptions, guid_re: &Regex, hex_re: &Regex) {
    if pkg.name.trim().is_empty() {
        ctx.error("name is empty");
    }
    if pkg.version.trim().is_empty() {
        ctx.error("version is empty");
    }
    check_version(ctx, "os_min", &pkg.os_min);
    check_version(ctx, "os_max", &pkg.os_max);
    if let Some(d) = &pkg.description {
        check_text(ctx, "description", d);
    }
    if let Some(w) = &pkg.warning {
        check_text(ctx, "warning", w);
    }
    if pkg.hidden && !pkg.profiles.is_empty() {
        ctx.warn("hidden packages should not be in profiles; they come in through depends");
    }
    if pkg.depends.contains(&pkg.id) {
        ctx.error("package depends on itself");
    }
    if pkg.conflicts.contains(&pkg.id) {
        ctx.error("package conflicts with itself");
    }
    let mut seen_arch = HashSet::new();
    for a in &pkg.os_arch {
        if *a == Arch::Neutral {
            ctx.error("os_arch lists Windows architectures; neutral is not one");
        }
        if !seen_arch.insert(a) {
            ctx.warn(format!("os_arch lists {a:?} twice"));
        }
    }
    let mut seen_profiles = HashSet::new();
    for p in &pkg.profiles {
        if !seen_profiles.insert(p) {
            ctx.warn(format!("profile {p:?} listed twice"));
        }
    }
    if let Some(h) = &pkg.homepage
        && !h.starts_with("https://") {
            ctx.warn("homepage should be https");
        }

    check_install(ctx, pkg);
    check_source(ctx, pkg, opts, hex_re);
    check_detect(ctx, pkg.arch, &pkg.detect, guid_re, 0);
}

fn check_install(ctx: &mut Ctx, pkg: &Package) {
    let i = &pkg.install;
    let is_dism = matches!(i.kind, InstallKind::DismFeature | InstallKind::DismCapability);
    match i.kind {
        InstallKind::Exe => {
            if i.args.is_none() {
                ctx.error("install.kind = exe needs install.args (use \"\" for none)");
            }
        }
        InstallKind::Msi => {
            if let Some(p) = &i.properties {
                let lower = p.to_ascii_lowercase();
                for flag in ["/qn", "/quiet", "/norestart", "/i ", "/l"] {
                    if lower.contains(flag) {
                        ctx.error(format!("install.properties must not contain {flag:?}; msiexec flags are added automatically"));
                    }
                }
            }
        }
        InstallKind::DismFeature => {
            if i.feature.as_deref().is_none_or(str::is_empty) {
                ctx.error("install.kind = dism-feature needs install.feature");
            }
            if let Some(h) = &i.source_hint {
                check_text(ctx, "install.source_hint", h);
            }
        }
        InstallKind::DismCapability => {
            if i.capability.as_deref().is_none_or(str::is_empty) {
                ctx.error("install.kind = dism-capability needs install.capability");
            }
        }
        InstallKind::Msu | InstallKind::Appx | InstallKind::Directx => {}
    }
    let stray = [
        ("args", i.args.is_some() && i.kind != InstallKind::Exe),
        ("properties", i.properties.is_some() && i.kind != InstallKind::Msi),
        ("provisioned", i.provisioned && i.kind != InstallKind::Appx),
        ("feature", i.feature.is_some() && i.kind != InstallKind::DismFeature),
        ("all", i.all.is_some() && i.kind != InstallKind::DismFeature),
        ("source_hint", i.source_hint.is_some() && i.kind != InstallKind::DismFeature),
        ("capability", i.capability.is_some() && i.kind != InstallKind::DismCapability),
    ];
    for (field, bad) in stray {
        if bad {
            ctx.error(format!("install.{field} does not apply to install.kind = {:?}", i.kind));
        }
    }
    if i.success_codes.is_empty() {
        ctx.error("install.success_codes is empty");
    }
    for c in &i.reboot_codes {
        if i.success_codes.contains(c) {
            ctx.error(format!("exit code {c} is in both success_codes and reboot_codes"));
        }
    }
    if i.timeout_sec < 30 {
        ctx.error("install.timeout_sec below 30 is too short for any installer");
    }
    if is_dism && pkg.source.is_some() {
        ctx.error("dism installs take no [package.source]");
    }
    if !is_dism && pkg.source.is_none() {
        ctx.error("[package.source] is required for this install.kind");
    }
}

fn expected_extensions(kind: InstallKind) -> &'static [&'static str] {
    match kind {
        InstallKind::Exe | InstallKind::Directx => &["exe"],
        InstallKind::Msi => &["msi"],
        InstallKind::Msu => &["msu"],
        InstallKind::Appx => &["appx", "appxbundle", "msix", "msixbundle"],
        InstallKind::DismFeature | InstallKind::DismCapability => &[],
    }
}

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

fn check_source(ctx: &mut Ctx, pkg: &Package, opts: ValidateOptions, hex_re: &Regex) {
    let Some(src) = &pkg.source else { return };
    for (field, url) in std::iter::once(("source.url", &src.url))
        .chain(src.fallback_urls.iter().map(|u| ("source.fallback_urls", u)))
    {
        if url.starts_with("http://") {
            ctx.warn(format!("{field} uses plain http; the file is still hash/signature checked"));
        } else if !url.starts_with("https://") {
            ctx.error(format!("{field} must be an http(s) URL"));
        }
    }
    if src.filename.is_empty()
        || src.filename.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
    {
        ctx.error("source.filename must be a bare file name");
    }
    let installer_name = match &src.extract {
        Some(x) => {
            if extension(&src.filename) != "zip" {
                ctx.error("source.filename of a zip archive should end in .zip");
            }
            x.entry.rsplit('/').next().unwrap_or(&x.entry).to_string()
        }
        None => src.filename.clone(),
    };
    let allowed = expected_extensions(pkg.install.kind);
    if !allowed.is_empty() && !allowed.contains(&extension(&installer_name).as_str()) {
        ctx.error(format!(
            "installer {installer_name:?} does not match install.kind = {:?} (expected .{})",
            pkg.install.kind,
            allowed.join(" / .")
        ));
    }
    match (&src.sha256, src.evergreen) {
        (Some(h), _) if !hex_re.is_match(h) => ctx.error("source.sha256 must be 64 lowercase hex characters"),
        (None, false) if opts.strict => ctx.error("source.sha256 is required for a non-evergreen source"),
        (None, false) => ctx.warn("source.sha256 not filled yet (run xtask catalog refresh)"),
        _ => {}
    }
    if src.evergreen && src.signer.is_empty() {
        ctx.error("evergreen sources need source.signer (the hash changes, the signature is what we check)");
    }
    if src.signer.iter().any(|s| s.trim().is_empty()) {
        ctx.error("source.signer has an empty entry");
    }
    if src.size == 0 {
        if opts.strict {
            ctx.error("source.size is 0");
        } else {
            ctx.warn("source.size not filled yet (run xtask catalog refresh)");
        }
    }
}

fn check_detect(ctx: &mut Ctx, arch: Arch, d: &Detect, guid_re: &Regex, depth: usize) {
    let need = |ctx: &mut Ctx, field: &str, present: bool| {
        if !present {
            ctx.error(format!("detect.kind = {:?} needs detect.{field}", d.kind));
        }
    };
    check_version(ctx, "detect.min_version", &d.min_version);
    match d.kind {
        DetectKind::Registry => {
            need(ctx, "hive", d.hive.is_some());
            need(ctx, "key", d.key.as_deref().is_some_and(|k| !k.is_empty()));
            if arch != Arch::Neutral || d.view.is_some() {
                need(ctx, "view", d.view.is_some());
            }
            if d.view == Some(RegView::Both) {
                ctx.error("detect.view = both is only for uninstall rules");
            }
            if let Some(k) = &d.key {
                if k.starts_with('\\') || k.to_ascii_uppercase().starts_with("HKEY") || k.to_ascii_uppercase().starts_with("HKLM") {
                    ctx.error("detect.key is relative to the hive: no leading backslash or hive name");
                }
                if k.to_ascii_lowercase().contains("wow6432node") {
                    ctx.warn("detect.key contains Wow6432Node; prefer view = \"32\" with the plain path");
                }
            }
            let needs_value = d.equals.is_some() || d.min_dword.is_some();
            if needs_value && d.value.is_none() {
                ctx.error("detect.equals / detect.min_dword need detect.value");
            }
            if d.version_value.is_some() && d.version_name_prefix.is_some() {
                ctx.error("use either detect.version_value or detect.version_name_prefix, not both");
            }
            if d.min_version.is_some() && d.version_value.is_none() && d.version_name_prefix.is_none() {
                ctx.error("registry detect.min_version needs detect.version_value or detect.version_name_prefix");
            }
        }
        DetectKind::Uninstall => {
            need(ctx, "display_name", d.display_name.is_some());
            need(ctx, "view", d.view.is_some());
            for (field, re) in [("display_name", &d.display_name), ("publisher", &d.publisher)] {
                if let Some(re) = re
                    && let Err(e) = Regex::new(re) {
                        ctx.error(format!("detect.{field} is not a valid regex: {e}"));
                    }
            }
        }
        DetectKind::MsiProduct => {
            if d.product_code.is_none() && d.upgrade_code.is_none() {
                ctx.error("detect.kind = msi-product needs detect.product_code or detect.upgrade_code");
            }
            for (field, code) in [("product_code", &d.product_code), ("upgrade_code", &d.upgrade_code)] {
                if let Some(code) = code
                    && !guid_re.is_match(code) {
                        ctx.error(format!("detect.{field} must be a braced GUID like {{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}}"));
                    }
            }
        }
        DetectKind::FileVersion => need(ctx, "path", d.path.as_deref().is_some_and(|p| !p.is_empty())),
        DetectKind::DismFeature => need(ctx, "feature", d.feature.is_some()),
        DetectKind::DismCapability => need(ctx, "capability", d.capability.is_some()),
        DetectKind::Appx => need(ctx, "family", d.family.is_some()),
        DetectKind::Any | DetectKind::All => {
            if d.rules.len() < 2 {
                ctx.error(format!("detect.kind = {:?} needs at least two detect.rules", d.kind));
            }
            if depth > 3 {
                ctx.error("detect rules nest deeper than 4 levels");
            }
            for r in &d.rules {
                check_detect(ctx, arch, r, guid_re, depth + 1);
            }
        }
        DetectKind::None => {}
    }
    if !matches!(d.kind, DetectKind::Any | DetectKind::All) && !d.rules.is_empty() {
        ctx.error("detect.rules only apply to kind = any / all");
    }
}

fn check_dependencies(catalog: &Catalog, issues: &mut Vec<Issue>) {
    let ids: HashMap<&str, &Package> = catalog.packages().map(|p| (p.id.as_str(), p)).collect();
    let file_of: HashMap<&str, &str> = catalog
        .categories
        .iter()
        .flat_map(|c| c.packages.iter().map(move |p| (p.id.as_str(), c.file.as_str())))
        .collect();

    for pkg in catalog.packages() {
        for other in &pkg.conflicts {
            let file = file_of[pkg.id.as_str()].to_string();
            match ids.get(other.as_str()) {
                None => issues.push(Issue {
                    severity: Severity::Error,
                    file,
                    package: Some(pkg.id.clone()),
                    message: format!("conflicts with unknown package {other:?}"),
                }),
                Some(o) if !o.conflicts.contains(&pkg.id) => issues.push(Issue {
                    severity: Severity::Warning,
                    file,
                    package: Some(pkg.id.clone()),
                    message: format!("conflict with {other:?} is not listed on both sides"),
                }),
                Some(o) => {
                    // A profile is installed as a whole; it cannot hold both sides.
                    if let Some(profile) = pkg.profiles.iter().find(|p| o.profiles.contains(p))
                        && pkg.id < o.id
                    {
                        issues.push(Issue {
                            severity: Severity::Error,
                            file,
                            package: Some(pkg.id.clone()),
                            message: format!("profile {profile:?} holds both {:?} and its conflict {other:?}", pkg.id),
                        });
                    }
                }
            }
            if pkg.depends.contains(other) {
                issues.push(Issue {
                    severity: Severity::Error,
                    file: file_of[pkg.id.as_str()].to_string(),
                    package: Some(pkg.id.clone()),
                    message: format!("{other:?} is both a dependency and a conflict"),
                });
            }
        }
        for dep in &pkg.depends {
            match ids.get(dep.as_str()) {
                None => issues.push(Issue {
                    severity: Severity::Error,
                    file: file_of[pkg.id.as_str()].to_string(),
                    package: Some(pkg.id.clone()),
                    message: format!("depends on unknown package {dep:?}"),
                }),
                // The planner sorts by order, so a dependency must sort first.
                Some(d) if d.order >= pkg.order => issues.push(Issue {
                    severity: Severity::Error,
                    file: file_of[pkg.id.as_str()].to_string(),
                    package: Some(pkg.id.clone()),
                    message: format!(
                        "dependency {dep:?} has order {} but must come before this package (order {})",
                        d.order, pkg.order
                    ),
                }),
                Some(_) => {}
            }
        }
    }

    // Cycle detection: depth-first search with an explicit visiting set.
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Visiting,
        Done,
    }
    fn visit<'a>(
        id: &'a str,
        ids: &HashMap<&'a str, &'a Package>,
        marks: &mut HashMap<&'a str, Mark>,
        stack: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        match marks.get(id) {
            Some(Mark::Done) => return None,
            Some(Mark::Visiting) => {
                let start = stack.iter().position(|s| *s == id).unwrap_or(0);
                let mut cycle: Vec<String> = stack[start..].iter().map(|s| s.to_string()).collect();
                cycle.push(id.to_string());
                return Some(cycle);
            }
            None => {}
        }
        marks.insert(id, Mark::Visiting);
        stack.push(id);
        if let Some(pkg) = ids.get(id) {
            for dep in &pkg.depends {
                if let Some((dep_id, _)) = ids.get_key_value(dep.as_str())
                    && let Some(c) = visit(dep_id, ids, marks, stack) {
                        return Some(c);
                    }
            }
        }
        stack.pop();
        marks.insert(id, Mark::Done);
        None
    }

    let mut marks = HashMap::new();
    for id in ids.keys() {
        let mut stack = Vec::new();
        if let Some(cycle) = visit(id, &ids, &mut marks, &mut stack) {
            issues.push(Issue {
                severity: Severity::Error,
                file: file_of[cycle[0].as_str()].to_string(),
                package: Some(cycle[0].clone()),
                message: format!("dependency cycle: {}", cycle.join(" -> ")),
            });
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::parse_category;

    const GOOD: &str = r#"
[category]
id = "vcredist"
name = { en = "Visual C++", tr = "Visual C++" }
order = 10
icon = "cpu"

[[package]]
id = "vc-2013-x64"
name = "Visual C++ 2013 Redistributable"
version = "12.0.40664.0"
arch = "x64"
profiles = ["recommended"]
order = 130
group = "vc-2013"

[package.source]
url = "https://aka.ms/highdpimfc2013x64enu"
filename = "vcredist_2013_x64.exe"
size = 7200744
sha256 = "abababababababababababababababababababababababababababababababab"
signer = ["Microsoft Corporation"]

[package.install]
kind = "exe"
args = "/install /quiet /norestart"
success_codes = [0, 1638]

[package.detect]
kind = "registry"
hive = "HKLM"
key = 'SOFTWARE\Microsoft\VisualStudio\12.0\VC\Runtimes\x64'
view = "64"
value = "Installed"
equals = 1
"#;

    fn catalog_of(texts: &[&str]) -> Catalog {
        Catalog {
            categories: texts
                .iter()
                .enumerate()
                .map(|(i, t)| parse_category(&format!("f{i}.toml"), t).unwrap())
                .collect(),
        }
    }

    fn errors(c: &Catalog, strict: bool) -> Vec<String> {
        validate(c, ValidateOptions { strict })
            .into_iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| i.message)
            .collect()
    }

    #[test]
    fn good_entry_has_no_errors_even_strict() {
        assert!(errors(&catalog_of(&[GOOD]), true).is_empty());
    }

    #[test]
    fn duplicate_ids_across_files() {
        let other = GOOD.replace("id = \"vcredist\"", "id = \"other\"").replace("group = \"vc-2013\"", "");
        let errs = errors(&catalog_of(&[GOOD, &other]), false);
        assert!(errs.iter().any(|e| e.contains("duplicate package id")), "{errs:?}");
    }

    #[test]
    fn unknown_dependency_and_wrong_extension() {
        let bad = GOOD
            .replace("group = \"vc-2013\"", "depends = [\"nope\"]")
            .replace("vcredist_2013_x64.exe", "vcredist_2013_x64.msi");
        let errs = errors(&catalog_of(&[&bad]), false);
        assert!(errs.iter().any(|e| e.contains("unknown package")), "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("does not match install.kind")), "{errs:?}");
    }

    #[test]
    fn missing_hash_is_error_only_in_strict() {
        let draft = GOOD.replace(
            "sha256 = \"abababababababababababababababababababababababababababababababab\"\n",
            "",
        );
        assert!(errors(&catalog_of(&[&draft]), false).is_empty());
        assert!(!errors(&catalog_of(&[&draft]), true).is_empty());
    }

    #[test]
    fn evergreen_needs_signer() {
        let bad = GOOD
            .replace("signer = [\"Microsoft Corporation\"]", "evergreen = true")
            .replace(
                "sha256 = \"abababababababababababababababababababababababababababababababab\"\n",
                "",
            );
        let errs = errors(&catalog_of(&[&bad]), false);
        assert!(errs.iter().any(|e| e.contains("need source.signer")), "{errs:?}");
    }

    #[test]
    fn dependency_must_have_lower_order() {
        let dep = GOOD
            .replace("id = \"vc-2013-x64\"", "id = \"dep\"")
            .replace("group = \"vc-2013\"", "")
            .replace("order = 130", "order = 140");
        let main = GOOD.replace("group = \"vc-2013\"", "depends = [\"dep\"]");
        let both = format!("{main}
{}", &dep[dep.find("[[package]]").unwrap()..]);
        let errs = errors(&catalog_of(&[&both]), false);
        assert!(errs.iter().any(|e| e.contains("must come before")), "{errs:?}");
    }

    #[test]
    fn profile_cannot_hold_both_sides_of_a_conflict() {
        let x64 = GOOD.replace("group = \"vc-2013\"", "conflicts = [\"vc-2013-x86\"]");
        let x86 = GOOD
            .replace("id = \"vc-2013-x64\"", "id = \"vc-2013-x86\"")
            .replace("group = \"vc-2013\"", "conflicts = [\"vc-2013-x64\"]");
        let both = format!("{x64}\n{}", &x86[x86.find("[[package]]").unwrap()..]);
        let errs = errors(&catalog_of(&[&both]), false);
        assert_eq!(errs.iter().filter(|e| e.contains("holds both")).count(), 1, "{errs:?}");
        let one = both.replacen("profiles = [\"recommended\"]", "profiles = []", 1);
        assert!(errors(&catalog_of(&[&one]), false).is_empty());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let bad = GOOD.replace("order = 130", "order = 130\ncolour = \"red\"");
        assert!(parse_category("f.toml", &bad).is_err());
    }

    #[test]
    fn dependency_cycle() {
        let a = GOOD.replace("group = \"vc-2013\"", "depends = [\"b\"]");
        let b = r#"
[category]
id = "other"
name = { en = "Other", tr = "Diğer" }
order = 20
icon = "box"

[[package]]
id = "b"
name = "B"
version = "1.0"
arch = "neutral"
order = 1
depends = ["vc-2013-x64"]

[package.install]
kind = "dism-feature"
feature = "NetFx3"

[package.detect]
kind = "dism-feature"
feature = "NetFx3"
"#;
        let errs = errors(&catalog_of(&[&a, b]), false);
        assert!(errs.iter().any(|e| e.contains("dependency cycle")), "{errs:?}");
    }
}
