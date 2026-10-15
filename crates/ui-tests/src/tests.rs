use std::cell::RefCell;
use std::rc::Rc;

use i_slint_backend_testing::{AccessibleRole, ElementHandle, init_no_event_loop};
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, LogicalSize, ModelRc, VecModel};

use super::*;

fn main_window() -> MainWindow {
    init_no_event_loop();
    let w = MainWindow::new().unwrap();
    w.window().set_size(LogicalSize::new(1040.0, 680.0));
    w.set_loading(false);
    w.show().unwrap();
    w
}

fn find(w: &MainWindow, label: &str, role: AccessibleRole) -> ElementHandle {
    ElementHandle::find_by_accessible_label(w, label)
        .find(|e| e.accessible_role() == Some(role))
        .unwrap_or_else(|| panic!("no {role:?} labeled {label:?}"))
}

/// One press and release in the middle of the element: a single click.
fn click(w: &MainWindow, label: &str, role: AccessibleRole) {
    let e = find(w, label, role);
    let size = e.size();
    assert!(size.width > 0.0 && size.height > 0.0, "{label:?} has no size");
    e.mock_single_click(PointerEventButton::Left);
}

fn recorder<T: Clone + 'static>() -> (Rc<RefCell<Vec<T>>>, impl Fn(T) + 'static) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let push = {
        let seen = seen.clone();
        move |v: T| seen.borrow_mut().push(v)
    };
    (seen, push)
}

#[test]
fn profile_chips_respond_to_the_first_click() {
    let w = main_window();
    let (seen, push) = recorder();
    w.on_choose_profile(push);
    // Each chip once, in a row: before the fix the first click on every chip
    // only moved keyboard focus and the second one selected it.
    click(&w, "Gaming", AccessibleRole::Checkbox);
    click(&w, "Developer", AccessibleRole::Checkbox);
    click(&w, "Everything", AccessibleRole::Checkbox);
    assert_eq!(*seen.borrow(), vec![1, 2, 4]);
}

#[test]
fn sidebar_responds_to_the_first_click() {
    let w = main_window();
    click(&w, "Offline package", AccessibleRole::Tab);
    assert_eq!(w.get_page(), 2);
    click(&w, "Settings", AccessibleRole::Tab);
    assert_eq!(w.get_page(), 4);
    click(&w, "Install", AccessibleRole::Tab);
    assert_eq!(w.get_page(), 0);
}

#[test]
fn buttons_respond_to_the_first_click() {
    let w = main_window();
    w.set_can_install(true);
    w.set_selected_count(3);
    let (installs, on_install) = recorder();
    w.on_install(move || on_install(()));
    let (downloads, on_download) = recorder();
    w.on_download_only(move || on_download(()));
    click(&w, "Gaming", AccessibleRole::Checkbox);
    click(&w, "Download only", AccessibleRole::Button);
    click(&w, "Install", AccessibleRole::Button);
    assert_eq!((installs.borrow().len(), downloads.borrow().len()), (1, 1));
}

#[test]
fn disabled_buttons_ignore_clicks() {
    let w = main_window();
    w.set_can_install(false);
    let (installs, on_install) = recorder();
    w.on_install(move || on_install(()));
    click(&w, "Install", AccessibleRole::Button);
    assert!(installs.borrow().is_empty());
}

#[test]
fn cancel_takes_one_click_and_says_what_happens() {
    let w = main_window();
    w.set_page(1);
    let state = w.global::<AppState>();
    state.set_busy(true);
    let (cancels, on_cancel) = recorder();
    state.on_cancel(move || on_cancel(()));
    click(&w, "Cancel", AccessibleRole::Button);
    click(&w, "Cancel", AccessibleRole::Button);
    assert_eq!(cancels.borrow().len(), 1, "a second click does nothing");
    assert_eq!(state.get_stage_text(), "Stopping after the current package...");
    assert_eq!(find(&w, "Cancel", AccessibleRole::Button).accessible_enabled(), Some(false));
}

#[test]
fn create_package_needs_a_destination() {
    let w = main_window();
    w.set_page(2);
    w.set_pack_count(4);
    let (packs, on_pack) = recorder();
    w.on_create_package(move || on_pack(()));
    w.set_pack_path("".into());
    click(&w, "Create package", AccessibleRole::Button);
    assert!(packs.borrow().is_empty());
    w.set_pack_path(r"D:\CAIOR-Offline.exe".into());
    click(&w, "Create package", AccessibleRole::Button);
    assert_eq!(packs.borrow().len(), 1);
}

#[test]
fn finished_card_can_be_closed() {
    init_no_event_loop();
    let card = MiniCard::new().unwrap();
    card.window().set_size(LogicalSize::new(360.0, 120.0));
    card.show().unwrap();
    let state = card.global::<AppState>();
    assert!(ElementHandle::find_by_accessible_label(&card, "Close").next().is_none(), "no close while running");
    state.set_finished(true);
    let (closes, on_close) = recorder();
    state.on_close_card(move || on_close(()));
    let (shows, on_show) = recorder();
    state.on_show_main(move || on_show(()));
    let close = ElementHandle::find_by_accessible_label(&card, "Close").next().expect("close button");
    close.mock_single_click(PointerEventButton::Left);
    ElementHandle::find_by_accessible_label(&card, "Show the main window")
        .next()
        .unwrap()
        .mock_single_click(PointerEventButton::Left);
    assert_eq!((closes.borrow().len(), shows.borrow().len()), (1, 1));
}

fn package(name: &str, enabled: bool) -> PackageRow {
    PackageRow { id: name.to_lowercase().into(), name: name.into(), enabled, ..Default::default() }
}

#[test]
fn package_rows_toggle_on_the_first_click() {
    let w = main_window();
    let rows = vec![
        PackageRow { is_header: true, category: "Visual C++".into(), ..Default::default() },
        package("VC 2013 x64", true),
        package("VC 2013 ARM64", false),
        package("VC 2015 x64", true),
    ];
    w.set_rows(ModelRc::new(VecModel::from(rows)));
    let (seen, push) = recorder();
    w.on_toggle(push);
    click(&w, "VC 2015 x64", AccessibleRole::Checkbox);
    click(&w, "VC 2013 ARM64", AccessibleRole::Checkbox);
    click(&w, "VC 2013 x64", AccessibleRole::Checkbox);
    assert_eq!(*seen.borrow(), vec![3, 1], "the disabled row stays out");
}

#[test]
fn turkish_translation_is_bundled() {
    let w = main_window();
    slint::select_bundled_translation("tr").unwrap();
    find(&w, "Kur", AccessibleRole::Button);
    find(&w, "Oyun", AccessibleRole::Checkbox);
    slint::select_bundled_translation("").unwrap();
    find(&w, "Install", AccessibleRole::Button);
}

/// Every `@tr("...")` in `ui/*.slint` needs a Turkish `msgstr` in
/// `i18n/tr/LC_MESSAGES/caior.po`; a missing one shows English in the Turkish UI.
#[test]
fn every_ui_string_has_a_turkish_translation() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let po = std::fs::read_to_string(root.join("i18n/tr/LC_MESSAGES/caior.po")).unwrap();
    let translations = parse_po(&po);
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(root.join("ui")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|x| x != "slint") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for msgid in tr_strings(&text) {
            if !translations.iter().any(|(id, s)| *id == msgid && !s.is_empty()) {
                missing.push(format!("{}: {msgid:?}", path.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    assert!(missing.is_empty(), "add these to caior.po:\n{}", missing.join("\n"));
}

/// The string literals right after `@tr(`, unescaped.
fn tr_strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("@tr(\"") {
        rest = &rest[at + 5..];
        let mut s = String::new();
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some((_, n)) = chars.next() {
                        s.push(match n {
                            'n' => '\n',
                            other => other,
                        });
                    }
                }
                '"' => {
                    rest = &rest[i + 1..];
                    break;
                }
                _ => s.push(c),
            }
        }
        out.push(s);
    }
    out
}

/// (msgid, msgstr) pairs; multi-line strings are joined.
fn parse_po(po: &str) -> Vec<(String, String)> {
    fn unquote(line: &str) -> String {
        line.trim().trim_start_matches('"').trim_end_matches('"').replace("\\\"", "\"").replace("\\n", "\n")
    }
    let mut pairs = Vec::new();
    let (mut id, mut text, mut in_id) = (String::new(), String::new(), false);
    for line in po.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("msgid ") {
            if !id.is_empty() {
                pairs.push((std::mem::take(&mut id), std::mem::take(&mut text)));
            }
            id = unquote(rest);
            text.clear();
            in_id = true;
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            text = unquote(rest);
            in_id = false;
        } else if line.starts_with('"') {
            if in_id {
                id.push_str(&unquote(line));
            } else {
                text.push_str(&unquote(line));
            }
        }
    }
    if !id.is_empty() {
        pairs.push((id, text));
    }
    pairs
}

#[test]
fn tr_scanner_reads_escapes() {
    assert_eq!(tr_strings(r#"a @tr("Error: {}", x) b @tr("Say \"hi\"")"#), vec!["Error: {}", "Say \"hi\""]);
    let po = "msgid \"\"\nmsgstr \"\"\n\nmsgid \"Install\"\nmsgstr \"Kur\"\n\nmsgid \"\"\n\"Long \"\n\"text\"\nmsgstr \"Uzun metin\"\n";
    assert_eq!(parse_po(po), vec![("Install".into(), "Kur".into()), ("Long text".into(), "Uzun metin".into())]);
}
