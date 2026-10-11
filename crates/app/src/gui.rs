//! The Slint GUI: package list, install progress and the bottom-right card.
//! All user-facing text lives in the .slint files (@tr); this module only
//! feeds numbers, names and states.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use caior_core::catalog::{Catalog, Package, Profile};
use caior_core::detect::{self, State, SystemInfo, applicability};
use caior_core::plan::{self, PlanOptions};
use caior_core::session::{self, Event, SessionOptions};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};


slint::include_modules!();

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProfileChoice {
    Recommended,
    Gaming,
    Developer,
    Legacy,
    Everything,
}

impl ProfileChoice {
    fn from_index(i: i32) -> Self {
        match i {
            1 => Self::Gaming,
            2 => Self::Developer,
            3 => Self::Legacy,
            4 => Self::Everything,
            _ => Self::Recommended,
        }
    }
    fn profile(self) -> Option<Profile> {
        match self {
            Self::Recommended => Some(Profile::Recommended),
            Self::Gaming => Some(Profile::Gaming),
            Self::Developer => Some(Profile::Developer),
            Self::Legacy => Some(Profile::Legacy),
            Self::Everything => None,
        }
    }
}

struct Gui {
    catalog: Catalog,
    sys: SystemInfo,
    states: HashMap<String, State>,
    selected: HashSet<String>,
    /// Package id of each list row; None for category headers.
    row_ids: Vec<Option<String>>,
    turkish: bool,
    /// Chip index of the Install page (5 = custom selection).
    profile: i32,
}

fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join("CAIOR")
}

fn session_options(download_only: bool) -> SessionOptions {
    SessionOptions {
        cache_dir: data_dir().join("cache"),
        log_root: data_dir().join("logs"),
        download_jobs: 3,
        download_only,
    }
}

fn status_code(state: Option<&State>, applicable: bool) -> i32 {
    if !applicable {
        return 4;
    }
    match state {
        Some(State::Installed { .. }) => 1,
        Some(State::Outdated { .. }) => 2,
        Some(State::Unknown { .. }) => 3,
        _ => 0,
    }
}

fn arch_label(p: &Package) -> &'static str {
    use caior_core::catalog::Arch;
    match p.arch {
        Arch::X86 => "x86",
        Arch::X64 => "x64",
        Arch::Arm64 => "ARM64",
        Arch::Neutral => "",
    }
}

fn size_mb(p: &Package) -> u64 {
    p.source.as_ref().map(|s| s.size).unwrap_or(0).div_ceil(1_048_576)
}

fn icon_for(icons: &Icons, name: &str) -> slint::Image {
    match name {
        "cpu" => icons.get_cpu(),
        "layers" => icons.get_layers(),
        "gamepad" | "gamepad-2" => icons.get_gamepad(),
        "app-window" => icons.get_app_window(),
        "coffee" => icons.get_coffee(),
        "database" => icons.get_database(),
        "terminal" => icons.get_terminal(),
        "archive" => icons.get_archive(),
        _ => icons.get_package(),
    }
}

impl Gui {
    fn applicable(&self, p: &Package) -> bool {
        applicability(p, &self.sys).is_ok()
    }

    fn rows(&mut self, icons: &Icons) -> Vec<PackageRow> {
        let mut rows = Vec::new();
        self.row_ids.clear();
        for c in &self.catalog.categories {
            let visible: Vec<&Package> = c.packages.iter().filter(|p| !p.hidden).collect();
            if visible.is_empty() {
                continue;
            }
            let installed = visible
                .iter()
                .filter(|p| matches!(self.states.get(&p.id), Some(State::Installed { .. })))
                .count();
            rows.push(PackageRow {
                is_header: true,
                category: c.category.name.get(if self.turkish { "tr" } else { "en" }).into(),
                icon: icon_for(icons, &c.category.icon),
                detail: format!("{installed} / {}", visible.len()).into(),
                ..Default::default()
            });
            self.row_ids.push(None);
            for p in visible {
                let applicable = applicability(p, &self.sys).is_ok();
                let state = self.states.get(&p.id);
                let version = match state {
                    Some(State::Outdated { version, wanted }) => format!("{version} \u{2192} {wanted}"),
                    _ => p.version.clone(),
                };
                let mut detail = vec![arch_label(p).to_string(), version];
                detail.retain(|d| !d.is_empty());
                let mb = size_mb(p);
                rows.push(PackageRow {
                    is_header: false,
                    id: p.id.clone().into(),
                    name: p.name.clone().into(),
                    detail: detail.join("  ").into(),
                    size: if p.source.is_none() { "Windows".into() } else if mb > 0 { format!("{mb} MB").into() } else { SharedString::new() },
                    status: status_code(state, applicable),
                    checked: applicable && self.selected.contains(&p.id),
                    enabled: applicable,
                    legacy: p.legacy,
                    ..Default::default()
                });
                self.row_ids.push(Some(p.id.clone()));
            }
        }
        rows
    }

    /// Profile packages that are applicable and not already installed.
    fn select_profile(&mut self, choice: ProfileChoice) {
        let ids = match choice.profile() {
            Some(p) => plan::profile_ids(&self.catalog, p, &self.sys),
            None => plan::all_ids(&self.catalog, &self.sys),
        };
        // Test aid: CAIOR_AUTOTEST_EXCLUDE=vc-2005-*,netfx-35 leaves packages out.
        let excludes: Vec<String> = std::env::var("CAIOR_AUTOTEST_EXCLUDE")
            .map(|v| v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
            .unwrap_or_default();
        let excluded = |id: &str| {
            excludes.iter().any(|p| match p.strip_suffix('*') {
                Some(prefix) => id.starts_with(prefix),
                None => p == id,
            })
        };
        self.selected = ids
            .into_iter()
            .filter(|id| !matches!(self.states.get(id), Some(State::Installed { .. })) && !excluded(id))
            .collect();
    }

    /// What an offline package gets: the whole profile (installed or not) plus
    /// any extra picks, or just the picks for a custom selection.
    fn pack_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = if (0..5).contains(&self.profile) {
            match ProfileChoice::from_index(self.profile).profile() {
                Some(p) => plan::profile_ids(&self.catalog, p, &self.sys),
                None => plan::all_ids(&self.catalog, &self.sys),
            }
        } else {
            Vec::new()
        };
        for id in &self.selected {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids.retain(|id| {
            self.catalog
                .package(id)
                .and_then(|p| p.source.as_ref())
                .is_some_and(|s| !s.online_only)
        });
        ids
    }

    fn pack_size(&self) -> (i32, i32) {
        let ids = self.pack_ids();
        let mb: u64 = ids.iter().filter_map(|id| self.catalog.package(id)).map(size_mb).sum();
        (ids.len() as i32, mb as i32)
    }

    fn selection_size(&self) -> (i32, i32) {
        let mb: u64 = self.selected.iter().filter_map(|id| self.catalog.package(id)).map(size_mb).sum();
        (self.selected.len() as i32, mb as i32)
    }
}

/// Mirrors AppState into both windows: each Slint window has its own globals.
#[derive(Clone)]
struct Windows {
    main: slint::Weak<MainWindow>,
    card: slint::Weak<MiniCard>,
}

impl Windows {
    fn state(&self, f: impl Fn(&AppState)) {
        if let Some(m) = self.main.upgrade() {
            f(&m.global::<AppState>());
        }
        if let Some(c) = self.card.upgrade() {
            f(&c.global::<AppState>());
        }
    }
    fn theme(&self, dark: bool) {
        if let Some(m) = self.main.upgrade() {
            m.global::<Theme>().set_dark(dark);
        }
        if let Some(c) = self.card.upgrade() {
            c.global::<Theme>().set_dark(dark);
        }
    }
}

fn place_card(card: &MiniCard) {
    let (_, _, right, bottom) = caior_core::desktop::work_area();
    let scale = card.window().scale_factor();
    let (w, h, margin) = ((360.0 * scale) as i32, (120.0 * scale) as i32, (12.0 * scale) as i32);
    card.window().set_position(slint::PhysicalPosition::new(right - w - margin, bottom - h - margin));
}

fn apply_language(turkish: bool) {
    let _ = slint::select_bundled_translation(if turkish { "tr" } else { "" });
}

pub fn run(catalog: Catalog) -> Result<()> {
    let turkish = caior_core::desktop::ui_language_is_turkish();
    let main = MainWindow::new()?;
    let card = MiniCard::new()?;
    // Slint only accepts a bundled language once a component exists.
    apply_language(turkish);
    let windows = Windows { main: main.as_weak(), card: card.as_weak() };
    windows.theme(!caior_core::desktop::apps_use_light_theme());

    let sys = SystemInfo::current();
    main.set_version(env!("CARGO_PKG_VERSION").into());
    main.set_system_text(
        format!(
            "{} {}  {}  build {}",
            sys.product_name.replace("Windows 10", if sys.is_windows_11() { "Windows 11" } else { "Windows 10" }),
            sys.display_version,
            format!("{:?}", sys.os_arch).to_lowercase(),
            sys.build()
        )
            .into(),
    );
    main.set_log_dir(data_dir().join("logs").display().to_string().into());
    let desktop = PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\Public".into())).join("Desktop");
    main.set_pack_path(desktop.join("CAIOR-Offline.exe").display().to_string().into());
    {
        let main_weak = main.as_weak();
        main.on_set_pack_format(move |f| {
            let Some(m) = main_weak.upgrade() else { return };
            m.set_pack_format(f);
            let path = m.get_pack_path().to_string();
            let fixed = match (f, path.to_ascii_lowercase().ends_with(".exe")) {
                (1, true) => path[..path.len() - 4].to_string(),
                (0, false) => format!("{}.exe", path.trim_end_matches(['\\', '/'])),
                _ => path,
            };
            m.set_pack_path(fixed.into());
        });
    }
    main.set_language(0);

    let gui = Rc::new(RefCell::new(Gui {
        catalog: catalog.clone(),
        sys: sys.clone(),
        states: HashMap::new(),
        selected: HashSet::new(),
        row_ids: Vec::new(),
        turkish,
        profile: 0,
    }));
    let rows_model = Rc::new(VecModel::<PackageRow>::default());
    main.set_rows(ModelRc::from(rows_model.clone()));
    let progress_model = Rc::new(VecModel::<ProgressRow>::default());
    main.set_progress_rows(ModelRc::from(progress_model.clone()));

    let refresh = {
        let gui = gui.clone();
        let rows_model = rows_model.clone();
        let main_weak = main.as_weak();
        move || {
            let Some(main) = main_weak.upgrade() else { return };
            let icons = main.global::<Icons>();
            let mut g = gui.borrow_mut();
            rows_model.set_vec(g.rows(&icons));
            let (n, mb) = g.selection_size();
            main.set_selected_count(n);
            main.set_selected_mb(mb);
            let (pn, pmb) = g.pack_size();
            main.set_pack_count(pn);
            main.set_pack_mb(pmb);
            main.set_can_install(n > 0 && !main.global::<AppState>().get_busy());
        }
    };
    let refresh = Rc::new(refresh);

    // Detection runs off the UI thread (WindowsProbe is created there); the
    // result is handed over through a mutex and a callback on the UI thread.
    let detected: Arc<std::sync::Mutex<Vec<(String, State)>>> = Arc::default();
    {
        let main_weak = main.as_weak();
        let catalog = catalog.clone();
        let sys = sys.clone();
        let detected = detected.clone();
        std::thread::spawn(move || {
            let probe = detect::WindowsProbe::new();
            let states: Vec<(String, State)> = detect::detect_all(&catalog, &sys, &probe)
                .into_iter()
                .filter_map(|s| match s.state {
                    detect::Status::Detected(st) => Some((s.id, st)),
                    detect::Status::NotApplicable { .. } => None,
                })
                .collect();
            *detected.lock().unwrap() = states;
            let _ = main_weak.upgrade_in_event_loop(|main| main.invoke_detection_done());
        });
    }

    {
        let gui = gui.clone();
        let refresh = refresh.clone();
        let main_weak = main.as_weak();
        let card_for_test = card.as_weak();
        main.on_detection_done(move || {
            {
                let mut g = gui.borrow_mut();
                for (id, st) in std::mem::take(&mut *detected.lock().unwrap()) {
                    g.states.insert(id, st);
                }
                g.select_profile(ProfileChoice::Recommended);
            }
            if let Some(m) = main_weak.upgrade() {
                m.set_loading(false);
            }
            refresh();
            schedule_snapshot(&main_weak);
            schedule_autotest(&main_weak, &card_for_test);
        });
    }

    {
        let gui = gui.clone();
        let refresh = refresh.clone();
        let main_weak = main.as_weak();
        main.on_choose_profile(move |i| {
            gui.borrow_mut().select_profile(ProfileChoice::from_index(i));
            gui.borrow_mut().profile = i;
            if let Some(m) = main_weak.upgrade() {
                m.set_profile(i);
            }
            refresh();
        });
    }

    {
        let gui = gui.clone();
        let refresh = refresh.clone();
        let main_weak = main.as_weak();
        main.on_toggle(move |i| {
            {
                let mut g = gui.borrow_mut();
                let Some(Some(id)) = g.row_ids.get(i as usize).cloned() else { return };
                let applicable = g.catalog.package(&id).is_some_and(|p| g.applicable(p));
                if !applicable {
                    return;
                }
                if !g.selected.remove(&id) {
                    g.selected.insert(id);
                }
                g.profile = 5;
            }
            if let Some(m) = main_weak.upgrade() {
                m.set_profile(5);
            }
            refresh();
        });
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_file: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    let start = {
        let gui = gui.clone();
        let windows = windows.clone();
        let progress_model = progress_model.clone();
        let cancel = cancel.clone();
        let cancel_file = cancel_file.clone();
        let main_weak = main.as_weak();
        // mode: 0 install, 1 download only, 2 offline package
        move |mode: u8| {
            let Some(main) = main_weak.upgrade() else { return };
            let download_only = mode != 0;
            let pack = mode == 2;
            let g = gui.borrow();
            let ids: Vec<String> = if pack { g.pack_ids() } else { g.selected.iter().cloned().collect() };
            let opts = PlanOptions { reinstall: pack };
            let Ok(plan) = plan::build(&g.catalog, &g.sys, &g.states, &ids, opts) else { return };
            progress_model.set_vec(
                plan.install
                    .iter()
                    .map(|i| ProgressRow {
                        id: i.id.clone().into(),
                        name: g.catalog.package(&i.id).map(|p| p.name.clone()).unwrap_or_default().into(),
                        ..Default::default()
                    })
                    .collect::<Vec<_>>(),
            );
            let total = plan.install.len() as i32;
            windows.state(|s| {
                s.set_busy(true);
                s.set_finished(false);
                s.set_done_count(0);
                s.set_total_count(total);
                s.set_overall(0.0);
                s.set_ok_count(0);
                s.set_failed_count(0);
                s.set_cancelled(false);
                s.set_declined(false);
                s.set_reboot_needed(false);
                s.set_download_only(download_only);
                s.set_pack_mode(pack);
                s.set_pack_result("".into());
                s.set_current_name("".into());
                s.set_stage(if download_only || g.sys.elevated { 0 } else { 1 });
            });
            main.set_can_install(false);
            main.set_page(1);
            cancel.store(false, Ordering::SeqCst);

            let elevated = g.sys.elevated;
            let catalog = g.catalog.clone();
            drop(g);
            let pack_folder = main.get_pack_format() == 1;
            let pack_out = PathBuf::from(main.get_pack_path().to_string());
            let main_weak = main.as_weak();
            let card_weak = windows.card.clone();
            let cancel = cancel.clone();
            let pipe = format!(
                "caior-gui-{}-{:08x}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0)
            );
            let cfile = std::env::temp_dir().join(format!("{pipe}.cancel"));
            *cancel_file.borrow_mut() = Some(cfile.clone());

            std::thread::spawn(move || {
                let forward = |e: Event| {
                    let main_weak = main_weak.clone();
                    let card_weak = card_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || on_event(&main_weak, &card_weak, e));
                };
                // CAIOR_FORCE_WORKER exercises the worker + pipe path where everything is
                // already elevated (Windows Sandbox), since UAC itself cannot be scripted.
                let force_worker = std::env::var_os("CAIOR_FORCE_WORKER").is_some();
                if pack {
                    // Download into the cache, then write the package; the final
                    // Finished event goes out only after the package is on disk.
                    let opts = session_options(true);
                    let mut summary = session::run(&catalog, &plan, &opts, &cancel, &|e| {
                        if !matches!(e, Event::Finished(_)) {
                            forward(e)
                        }
                    });
                    let mut result_path = String::new();
                    if summary.failed == 0 && !summary.cancelled {
                        let (mw, cw) = (main_weak.clone(), card_weak.clone());
                        let _ = slint::invoke_from_event_loop(move || Windows { main: mw, card: cw }.state(|s| s.set_stage(4)));
                        let ids: Vec<String> = plan.install.iter().map(|i| i.id.clone()).collect();
                        let cache = caior_core::fetch::Cache::new(caior_core::bundle::effective_cache(&opts.cache_dir));
                        let format = if pack_folder { caior_core::bundle::Format::Folder } else { caior_core::bundle::Format::Exe };
                        match caior_core::bundle::pack(&catalog, &ids, &cache, format, &pack_out) {
                            Ok((count, path)) => {
                                summary.downloaded = count;
                                result_path = path.display().to_string();
                            }
                            Err(e) => {
                                summary.failed += 1;
                                forward(Event::Failed { id: "offline-package".into(), error: e.to_string() });
                            }
                        }
                    }
                    let (mw, cw) = (main_weak.clone(), card_weak.clone());
                    let _ = slint::invoke_from_event_loop(move || {
                        Windows { main: mw.clone(), card: cw.clone() }.state(|s| s.set_pack_result(result_path.clone().into()));
                        on_event(&mw, &cw, Event::Finished(summary));
                    });
                } else if download_only || (elevated && !force_worker) {
                    let summary = session::run(&catalog, &plan, &session_options(download_only), &cancel, &forward);
                    let _ = summary;
                } else {
                    let mut args = vec!["worker".to_string(), "--pipe".into(), pipe.clone(), "--cancel-file".into(), cfile.display().to_string()];
                    for i in &plan.install {
                        args.push("--id".into());
                        args.push(i.id.clone());
                    }
                    let mut finished = false;
                    let result = caior_core::elevate::run_elevated(&pipe, &args, |line| {
                        if let Ok(e) = serde_json::from_str::<Event>(line) {
                            finished |= matches!(e, Event::Finished(_));
                            forward(e);
                        }
                    });
                    if !finished {
                        let declined = matches!(result, Err(caior_core::elevate::ElevateError::Declined));
                        let main_weak = main_weak.clone();
                        let card_weak = card_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            let w = Windows { main: main_weak, card: card_weak };
                            w.state(|s| {
                                s.set_busy(false);
                                s.set_finished(true);
                                s.set_declined(declined);
                                if !declined {
                                    s.set_failed_count(s.get_failed_count().max(1));
                                }
                            });
                            if let Some(m) = w.main.upgrade() {
                                m.invoke_session_ended();
                            }
                        });
                    }
                }
                let _ = std::fs::remove_file(&cfile);
            });
        }
    };
    let start = Rc::new(start);
    {
        let start = start.clone();
        main.on_install(move || start(0));
    }
    {
        let start = start.clone();
        main.on_download_only(move || start(1));
    }
    {
        let start = start.clone();
        main.on_create_package(move || start(2));
    }

    // After a session: detect again so the list shows the new state.
    {
        let gui = gui.clone();
        let refresh = refresh.clone();
        let main_weak = main.as_weak();
        main.on_session_ended(move || {
            let Some(main) = main_weak.upgrade() else { return };
            let catalog = gui.borrow().catalog.clone();
            let sys = gui.borrow().sys.clone();
            let probe = detect::WindowsProbe::new();
            let mut g = gui.borrow_mut();
            for s in detect::detect_all(&catalog, &sys, &probe) {
                if let detect::Status::Detected(st) = s.state {
                    g.states.insert(s.id, st);
                }
            }
            let profile = main.get_profile();
            if profile != 5 {
                g.select_profile(ProfileChoice::from_index(profile));
            } else {
                let states = g.states.clone();
                g.selected.retain(|id| !matches!(states.get(id), Some(State::Installed { .. })));
            }
            drop(g);
            refresh();
        });
    }

    {
        let main_weak = main.as_weak();
        main.on_back_to_list(move || {
            if let Some(m) = main_weak.upgrade() {
                m.global::<AppState>().set_finished(false);
                m.set_page(0);
            }
        });
    }

    // Hide the main window and follow the install in the bottom-right card.
    {
        let windows = windows.clone();
        main.on_minimize_to_card(move || {
            if let (Some(m), Some(c)) = (windows.main.upgrade(), windows.card.upgrade()) {
                let _ = c.show();
                place_card(&c);
                let _ = m.hide();
            }
        });
    }
    {
        let windows = windows.clone();
        let show_main = move || {
            if let (Some(m), Some(c)) = (windows.main.upgrade(), windows.card.upgrade()) {
                let _ = m.show();
                let _ = c.hide();
            }
        };
        let show_main2 = show_main.clone();
        main.global::<AppState>().on_show_main(show_main);
        card.global::<AppState>().on_show_main(show_main2);
    }

    // Closing the window during an install keeps it running in the card.
    {
        let windows = windows.clone();
        main.window().on_close_requested(move || {
            let busy = windows.main.upgrade().is_some_and(|m| m.global::<AppState>().get_busy());
            if busy {
                if let (Some(m), Some(c)) = (windows.main.upgrade(), windows.card.upgrade()) {
                    let _ = c.show();
                    place_card(&c);
                    let _ = m.hide();
                }
                slint::CloseRequestResponse::HideWindow
            } else {
                let _ = slint::quit_event_loop();
                slint::CloseRequestResponse::HideWindow
            }
        });
    }

    let cancel_handler = {
        let cancel = cancel.clone();
        let cancel_file = cancel_file.clone();
        move || {
            cancel.store(true, Ordering::SeqCst);
            if let Some(f) = cancel_file.borrow().as_ref() {
                let _ = std::fs::write(f, b"cancel");
            }
        }
    };
    {
        let c1 = cancel_handler.clone();
        main.global::<AppState>().on_cancel(c1);
        card.global::<AppState>().on_cancel(cancel_handler);
    }

    let restart = || {
        let _ = std::process::Command::new("shutdown.exe").args(["/r", "/t", "5", "/c", "CAIOR"]).spawn();
    };
    main.global::<AppState>().on_restart_now(restart);
    card.global::<AppState>().on_restart_now(restart);

    {
        let main_weak = main.as_weak();
        main.global::<AppState>().on_show_pack_result(move || {
            if let Some(m) = main_weak.upgrade() {
                let path = m.global::<AppState>().get_pack_result().to_string();
                let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{path}")).spawn();
            }
        });
    }
    main.on_open_logs(|| {
        let dir = data_dir().join("logs");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
    });

    {
        let windows = windows.clone();
        let main_weak = main.as_weak();
        main.on_set_theme(move |mode| {
            if let Some(m) = main_weak.upgrade() {
                m.set_theme_mode(mode);
            }
            windows.theme(match mode {
                1 => true,
                2 => false,
                _ => !caior_core::desktop::apps_use_light_theme(),
            });
        });
    }
    {
        let gui = gui.clone();
        let refresh = refresh.clone();
        let main_weak = main.as_weak();
        main.on_set_language(move |lang| {
            let turkish = match lang {
                1 => false,
                2 => true,
                _ => caior_core::desktop::ui_language_is_turkish(),
            };
            apply_language(turkish);
            gui.borrow_mut().turkish = turkish;
            if let Some(m) = main_weak.upgrade() {
                m.set_language(lang);
            }
            refresh();
        });
    }

    main.show()?;
    slint::run_event_loop_until_quit()?;
    Ok(())
}

/// Applies one session event to both windows.
fn on_event(main_weak: &slint::Weak<MainWindow>, card_weak: &slint::Weak<MiniCard>, e: Event) {
    let windows = Windows { main: main_weak.clone(), card: card_weak.clone() };
    let Some(main) = main_weak.upgrade() else { return };
    let model = main.get_progress_rows();
    let find = |id: &str| (0..model.row_count()).find(|&i| model.row_data(i).is_some_and(|r| r.id == id));
    let set_row = |id: &str, state: i32, percent: i32, code: &str| {
        if let Some(i) = find(id) {
            let mut r = model.row_data(i).unwrap();
            r.state = state;
            r.percent = percent;
            r.code = code.into();
            model.set_row_data(i, r);
        }
    };
    let finished_rows = || (0..model.row_count()).filter(|&i| model.row_data(i).is_some_and(|r| r.state >= 4)).count() as f32;
    match e {
        Event::Started { total, .. } => windows.state(|s| {
            s.set_total_count(total as i32);
            s.set_stage(2);
        }),
        Event::Downloading { id, done, total } => {
            let pct = total.filter(|t| *t > 0).map(|t| (done * 100 / t) as i32).unwrap_or(0);
            set_row(&id, 1, pct, "");
            let state = main.global::<AppState>();
            if state.get_stage() != 3 {
                windows.state(|s| {
                    s.set_stage(2);
                    s.set_stage_percent(pct);
                    s.set_current_name(model.row_data(find(&id).unwrap_or(0)).map(|r| r.name).unwrap_or_default());
                });
            }
        }
        Event::Downloaded { id, .. } => {
            set_row(&id, 2, 100, "");
            let rows = model.row_count().max(1) as f32;
            let ready = (0..model.row_count()).filter(|&i| model.row_data(i).is_some_and(|r| r.state >= 2)).count() as f32;
            let dl_only = main.global::<AppState>().get_download_only();
            if dl_only {
                windows.state(|s| {
                    s.set_done_count(ready as i32);
                    s.set_ok_count(ready as i32);
                    s.set_overall(ready / rows);
                });
            }
        }
        Event::InstallStarted { id, index, total } => {
            set_row(&id, 3, 0, "");
            let name = model.row_data(find(&id).unwrap_or(0)).map(|r| r.name).unwrap_or_default();
            windows.state(|s| {
                s.set_stage(3);
                s.set_current_name(name.clone());
                s.set_done_count(index as i32 - 1);
                s.set_total_count(total as i32);
            });
        }
        Event::InstallFinished { id, outcome, code, .. } => {
            let (state, text) = match outcome.as_str() {
                "success" => (4, String::new()),
                "reboot" => (5, String::new()),
                "timed-out" => (6, "timeout".to_string()),
                _ => (6, code.map(|c| c.to_string()).unwrap_or_default()),
            };
            set_row(&id, state, 100, &text);
            let rows = model.row_count().max(1) as f32;
            let done = finished_rows();
            windows.state(|s| {
                s.set_done_count(done as i32);
                s.set_overall(done / rows);
                if state == 6 {
                    s.set_failed_count(s.get_failed_count() + 1);
                } else {
                    s.set_ok_count(s.get_ok_count() + 1);
                }
                if state == 5 {
                    s.set_reboot_needed(true);
                }
            });
        }
        Event::Failed { id, error } => {
            let short: String = error.chars().take(60).collect();
            set_row(&id, 6, 0, &short);
            windows.state(|s| s.set_failed_count(s.get_failed_count() + 1));
        }
        Event::Skipped { id, .. } => {
            if find(&id).is_some() {
                set_row(&id, 7, 0, "");
            }
        }
        Event::Finished(summary) => {
            windows.state(|s| {
                s.set_busy(false);
                s.set_finished(true);
                s.set_overall(1.0);
                s.set_cancelled(summary.cancelled);
                s.set_reboot_needed(summary.reboot_required);
                if s.get_download_only() {
                    s.set_ok_count(summary.downloaded as i32);
                } else {
                    s.set_ok_count(summary.installed as i32);
                }
                s.set_failed_count(summary.failed as i32);
            });
            main.invoke_session_ended();
            // A clean run without restart clears the card after a while.
            if summary.failed == 0 && !summary.reboot_required && !summary.cancelled {
                let card_weak = card_weak.clone();
                slint::Timer::single_shot(std::time::Duration::from_secs(8), move || {
                    if let Some(c) = card_weak.upgrade() {
                        let _ = c.hide();
                    }
                });
            }
        }
    }
}

/// Developer aid: with CAIOR_SNAPSHOT=<file.png> (and optionally CAIOR_PAGE=<n>),
/// render the window once detection is done, save it and quit.
fn schedule_snapshot(main_weak: &slint::Weak<MainWindow>) {
    let Ok(path) = std::env::var("CAIOR_SNAPSHOT") else { return };
    if let (Some(m), Ok(page)) = (main_weak.upgrade(), std::env::var("CAIOR_PAGE")) {
        m.set_page(page.parse().unwrap_or(0));
    }
    let main_weak = main_weak.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(600), move || {
        let Some(m) = main_weak.upgrade() else { return };
        if let Ok(buf) = m.window().take_snapshot()
            && let Ok(file) = std::fs::File::create(&path) {
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), buf.width(), buf.height());
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                if let Ok(mut w) = enc.write_header() {
                    let _ = w.write_image_data(buf.as_bytes());
                }
            }
        let _ = slint::quit_event_loop();
    });
}

fn save_png(buf: &slint::SharedPixelBuffer<slint::Rgba8Pixel>, path: &std::path::Path) {
    if let Ok(file) = std::fs::File::create(path) {
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), buf.width(), buf.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = enc.write_header() {
            let _ = w.write_image_data(buf.as_bytes());
        }
    }
}

/// Test aid for Windows Sandbox, where nobody can click: with CAIOR_AUTOTEST=<dir>
/// the GUI starts the install by itself (CAIOR_AUTOTEST_PROFILE=<0-4>, default
/// recommended), saves screenshots of the progress page, the bottom-right card
/// and the final state into <dir>, then quits.
fn schedule_autotest(main_weak: &slint::Weak<MainWindow>, card_weak: &slint::Weak<MiniCard>) {
    let Ok(dir) = std::env::var("CAIOR_AUTOTEST") else { return };
    let dir = PathBuf::from(dir);
    let _ = std::fs::create_dir_all(&dir);
    let Some(main) = main_weak.upgrade() else { return };
    let profile = std::env::var("CAIOR_AUTOTEST_PROFILE").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    main.invoke_choose_profile(profile);
    if let Ok(buf) = main.window().take_snapshot() {
        save_png(&buf, &dir.join("1-list.png"));
    }
    main.invoke_install();

    let step = Rc::new(std::cell::Cell::new(0u32));
    let timer = Rc::new(slint::Timer::default());
    let (mw, cw, t2) = (main_weak.clone(), card_weak.clone(), timer.clone());
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(2), move || {
        let (Some(m), Some(c)) = (mw.upgrade(), cw.upgrade()) else { return };
        let n = step.get() + 1;
        step.set(n);
        let state = m.global::<AppState>();
        match n {
            3 => {
                if let Ok(buf) = m.window().take_snapshot() {
                    save_png(&buf, &dir.join("2-progress.png"));
                }
                m.invoke_minimize_to_card();
            }
            5 => {
                if let Ok(buf) = c.window().take_snapshot() {
                    save_png(&buf, &dir.join("3-card.png"));
                }
            }
            _ if n > 5 && state.get_finished() => {
                if let Ok(buf) = c.window().take_snapshot() {
                    save_png(&buf, &dir.join("4-card-finished.png"));
                }
                state.invoke_show_main();
                if let Ok(buf) = m.window().take_snapshot() {
                    save_png(&buf, &dir.join("5-finished.png"));
                }
                let _ = std::fs::write(dir.join("DONE-GUI"), "done");
                t2.stop();
                let _ = slint::quit_event_loop();
            }
            _ if n > 900 => {
                let _ = std::fs::write(dir.join("DONE-GUI"), "timeout");
                let _ = slint::quit_event_loop();
            }
            _ => {}
        }
    });
    // Keep the timer alive for the rest of the run.
    std::mem::forget(timer);
}
