//! Conventional modal dialogs.

use crate::datatable::{DataTable, Row};
use crate::theme::Theme;
use be_core::codes;
use be_core::policy::{ManualCheckRecord, ManualCheckStatus, ReleaseState};
use be_core::profile::Profile;
use be_core::time::UtcMillis;
use be_storage::Unit;
use fltk::button::Button;
use fltk::dialog;
use fltk::enums::{Align, FrameType};
use fltk::frame::Frame;
use fltk::group::Flex;
use fltk::input::{Input, MultilineInput};
use fltk::menu::Choice;
use fltk::prelude::*;
use fltk::text::{TextBuffer, TextDisplay};
use fltk::window::Window;
use std::cell::RefCell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub fn alert(msg: &str) {
    dialog::alert_default(msg);
}

pub fn message(msg: &str) {
    dialog::message_title_default("BE Validation");
    dialog::message_default(msg);
}

/// Returns Some(0) for `yes`, Some(1) for `no`.
pub fn confirm(msg: &str, yes: &str, no: &str) -> Option<i32> {
    // FLTK's choice2 places button 0 rightmost-left; map explicitly.
    match dialog::choice2_default(msg, no, yes, "") {
        Some(1) => Some(0),
        Some(0) => Some(1),
        other => other,
    }
}

pub fn input(prompt: &str, default: &str) -> Option<String> {
    dialog::input_default(prompt, default)
}

pub fn open_file(title: &str, filter: &str) -> Option<PathBuf> {
    let mut fc = dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseFile);
    fc.set_title(title);
    fc.set_filter(filter);
    fc.show();
    let p = fc.filename();
    if p.as_os_str().is_empty() {
        None
    } else {
        Some(p)
    }
}

pub fn save_file(title: &str, default_name: &str) -> Option<PathBuf> {
    let mut fc = dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseSaveFile);
    fc.set_title(title);
    fc.set_preset_file(default_name);
    fc.set_option(dialog::NativeFileChooserOptions::SaveAsConfirm);
    fc.show();
    let p = fc.filename();
    if p.as_os_str().is_empty() {
        None
    } else {
        Some(p)
    }
}

pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("out")
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(tmp, path)
}

/// Open a local file with the desktop's default viewer (for printing).
pub fn open_in_system_viewer(path: &Path) -> bool {
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(path).spawn();
    #[cfg(windows)]
    let r = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(path).spawn();
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    let r: std::io::Result<std::process::Child> = Err(std::io::Error::other("unsupported"));
    r.is_ok()
}

fn modal(title: &str, w: i32, h: i32) -> Window {
    let mut win = Window::default().with_size(w, h).with_label(title);
    win.make_modal(true);
    win.size_range(w / 2, h / 2, 0, 0);
    win
}

fn run_modal(win: &mut Window) {
    win.end();
    win.make_resizable(true);
    win.show();
    while win.shown() {
        fltk::app::wait();
    }
}

fn labeled_input(col: &mut Flex, label: &str, value: &str) -> Input {
    let mut row = Flex::default().row();
    col.fixed(&row, 26);
    let mut l = Frame::default().with_label(label);
    l.set_align(Align::Right | Align::Inside);
    row.fixed(&l, 170);
    let mut i = Input::default();
    i.set_value(value);
    row.end();
    i
}

fn ok_cancel(col: &mut Flex, ok_label: &str) -> (Button, Button) {
    let mut row = Flex::default().row();
    col.fixed(&row, 30);
    Frame::default();
    let mut ok = Button::default().with_label(ok_label);
    ok.set_frame(FrameType::ThinUpBox);
    row.fixed(&ok, 110);
    let mut cancel = Button::default().with_label("Cancel");
    cancel.set_frame(FrameType::ThinUpBox);
    row.fixed(&cancel, 90);
    row.end();
    (ok, cancel)
}

pub fn new_unit(operator: &str) -> Option<Unit> {
    let mut win = modal("New Unit Record", 560, 300);
    let mut col = Flex::default_fill().column();
    col.set_margin(10);
    col.set_pad(6);
    let mut hint = Frame::default().with_label(
        "Create a manufacturing record. Required tests and manual checks must pass before release.",
    );
    hint.set_align(Align::Left | Align::Inside | Align::Wrap);
    col.fixed(&hint, 36);
    let serial = labeled_input(&mut col, "Internal serial *", "");
    let config = labeled_input(&mut col, "Configuration / BOM ref.", "");
    let chassis = labeled_input(&mut col, "Chassis revision", "");
    let order = labeled_input(&mut col, "Order reference", "");
    let tech = labeled_input(&mut col, "Assembly technician", operator);
    Frame::default();
    let (mut ok, mut cancel) = ok_cancel(&mut col, "Create");
    col.end();
    let out: Rc<RefCell<Option<Unit>>> = Rc::new(RefCell::new(None));
    {
        let (out, mut w) = (out.clone(), win.clone());
        ok.set_callback(move |_| {
            let s = serial.value().trim().to_string();
            if s.is_empty() || s.len() > 64 || s.chars().any(|c| c.is_control()) {
                alert("Serial is required (max 64 characters, no control characters).");
                return;
            }
            let o = |i: &Input| Some(i.value().trim().to_string()).filter(|v| !v.is_empty());
            *out.borrow_mut() = Some(Unit {
                serial: s,
                chassis_revision: o(&chassis),
                order_ref: o(&order),
                configuration: o(&config),
                bom_json: None,
                assembly_technician: o(&tech),
                created_utc: UtcMillis::now(),
                release_state: ReleaseState::AwaitingTest,
            });
            w.hide();
        });
    }
    {
        let mut w = win.clone();
        cancel.set_callback(move |_| w.hide());
    }
    run_modal(&mut win);
    out.borrow_mut().take()
}

pub fn manual_checks(
    profile: &Profile,
    existing: &[ManualCheckRecord],
    operator: &str,
    run_id: i64,
) -> Option<Vec<ManualCheckRecord>> {
    let n = profile.manual_checks.len() as i32;
    let mut win = modal(
        &format!("Manual Inspection Sign-off — run {run_id}"),
        820,
        (150 + n * 30).min(800),
    );
    let mut col = Flex::default_fill().column();
    col.set_margin(10);
    col.set_pad(4);
    let mut hint = Frame::default().with_label("Record only what was physically inspected. Instrument-based items must reference the approved external procedure/report in the note.");
    hint.set_align(Align::Left | Align::Inside | Align::Wrap);
    col.fixed(&hint, 34);
    let op = labeled_input(&mut col, "Inspector identity *", operator);
    let mut rows = Vec::new();
    for c in &profile.manual_checks {
        let mut row = Flex::default().row();
        col.fixed(&row, 26);
        let cur = existing
            .iter()
            .rev()
            .find(|e| e.check_id == c.id)
            .map(|e| e.status);
        let mut l = Frame::default().with_label(
            &format!(
                "{}{}{}",
                c.label,
                if c.required { " *" } else { "" },
                if c.instrument { " [instrument]" } else { "" }
            )
            .replace('@', "@@"),
        );
        l.set_align(Align::Left | Align::Inside | Align::Clip);
        let mut ch = Choice::default();
        for o in ["Not done", "Pass", "Fail", "N/A"] {
            ch.add_choice(o);
        }
        ch.set_value(match cur {
            Some(ManualCheckStatus::Pass) => 1,
            Some(ManualCheckStatus::Fail) => 2,
            Some(ManualCheckStatus::NotApplicable) => 3,
            _ => 0,
        });
        row.fixed(&ch, 100);
        let note = Input::default();
        row.fixed(&note, 200);
        row.end();
        rows.push((c.id.clone(), ch, note, cur));
    }
    Frame::default();
    let (mut ok, mut cancel) = ok_cancel(&mut col, "Sign off");
    col.end();
    let out: Rc<RefCell<Option<Vec<ManualCheckRecord>>>> = Rc::new(RefCell::new(None));
    {
        let (out, mut w) = (out.clone(), win.clone());
        ok.set_callback(move |_| {
            let who = op.value().trim().to_string();
            if who.is_empty() {
                alert("Inspector identity is required.");
                return;
            }
            let mut recs = Vec::new();
            for (id, ch, note, cur) in &rows {
                let st = match ch.value() {
                    1 => ManualCheckStatus::Pass,
                    2 => ManualCheckStatus::Fail,
                    3 => ManualCheckStatus::NotApplicable,
                    _ => ManualCheckStatus::NotDone,
                };
                if Some(st) != *cur && !(st == ManualCheckStatus::NotDone && cur.is_none()) {
                    recs.push(ManualCheckRecord {
                        check_id: id.clone(),
                        status: st,
                        operator: who.clone(),
                        note: note.value(),
                    });
                }
            }
            *out.borrow_mut() = Some(recs);
            w.hide();
        });
    }
    {
        let mut w = win.clone();
        cancel.set_callback(move |_| w.hide());
    }
    run_modal(&mut win);
    out.borrow_mut().take()
}

pub fn service_record() -> Option<(String, String, Vec<String>)> {
    let mut win = modal("Add Service Record", 560, 280);
    let mut col = Flex::default_fill().column();
    col.set_margin(10);
    col.set_pad(6);
    let mut row = Flex::default().row();
    col.fixed(&row, 26);
    let mut l = Frame::default().with_label("Kind");
    l.set_align(Align::Right | Align::Inside);
    row.fixed(&l, 170);
    let mut kind = Choice::default();
    for k in [
        "repair",
        "part-replacement",
        "upgrade",
        "inspection",
        "rework",
    ] {
        kind.add_choice(k);
    }
    kind.set_value(0);
    row.end();
    let mut dl = Frame::default().with_label("Description *");
    dl.set_align(Align::Left | Align::Inside);
    col.fixed(&dl, 20);
    let desc = MultilineInput::default();
    let parts = labeled_input(&mut col, "Parts (comma separated)", "");
    let (mut ok, mut cancel) = ok_cancel(&mut col, "Add");
    col.end();
    let out = Rc::new(RefCell::new(None));
    {
        let (out, mut w) = (out.clone(), win.clone());
        ok.set_callback(move |_| {
            if desc.value().trim().is_empty() {
                alert("Description is required.");
                return;
            }
            let p: Vec<String> = parts
                .value()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            *out.borrow_mut() = Some((
                kind.choice().unwrap_or("repair".into()),
                desc.value().trim().to_string(),
                p,
            ));
            w.hide();
        });
    }
    {
        let mut w = win.clone();
        cancel.set_callback(move |_| w.hide());
    }
    run_modal(&mut win);
    out.borrow_mut().take()
}

pub fn text_window(title: &str, text: &str, th: &Theme) {
    let mut win = Window::default().with_size(900, 560).with_label(title);
    let mut col = Flex::default_fill().column();
    col.set_margin(8);
    let mut d = TextDisplay::default();
    let mut b = TextBuffer::default();
    b.set_text(text);
    d.set_buffer(b);
    d.set_text_font(fltk::enums::Font::Courier);
    d.set_text_size(12);
    d.set_color(th.content);
    d.set_text_color(th.text);
    col.end();
    win.end();
    win.make_resizable(true);
    win.show();
}

pub fn fault_catalog(th: &Theme) {
    let mut win = Window::default()
        .with_size(980, 600)
        .with_label(&format!("Fault Code Catalog v{}", codes::CATALOG_VERSION));
    let mut col = Flex::default_fill().column();
    col.set_margin(8);
    col.set_pad(6);
    let mut t = DataTable::new(
        &["Code", "Severity", "Title", "Meaning"],
        &[110, 80, 300, 460],
        *th,
    );
    t.set_rows(
        codes::CATALOG
            .iter()
            .map(|c| {
                Row::new(
                    c.code,
                    vec![
                        c.code.into(),
                        format!("{:?}", c.severity),
                        c.title.into(),
                        c.meaning.into(),
                    ],
                )
            })
            .collect(),
    );
    let mut d = TextDisplay::default();
    let b = TextBuffer::default();
    d.set_buffer(b.clone());
    d.wrap_mode(fltk::text::WrapMode::AtBounds, 0);
    d.set_color(th.content);
    d.set_text_color(th.text);
    col.fixed(&d, 170);
    col.end();
    let mut b2 = b.clone();
    t.set_on_select(move |code| {
        if let Some(c) = codes::lookup(code) {
            b2.set_text(&format!(
                "{} — {}\n{}\n\nPossible causes (hypotheses):\n  • {}\n\nNext safe steps:\n  • {}",
                c.code,
                c.title,
                c.meaning,
                c.possible_causes.join("\n  • "),
                c.next_steps.join("\n  • ")
            ));
        }
    });
    win.end();
    win.make_resizable(true);
    win.show();
}

pub fn profile_details(p: &Profile, fixture: bool, th: &Theme) {
    let mut win = Window::default()
        .with_size(980, 560)
        .with_label(&format!("Profile: {} {}", p.name, p.version));
    let mut col = Flex::default_fill().column();
    col.set_margin(8);
    col.set_pad(6);
    let mut f = Frame::default().with_label(
        &format!(
            "{}\nPlanned duration: {} | Release-qualifying: {} | Warnings accepted: {}",
            p.description,
            p.planned_minutes
                .map(|m| format!("{}–{} min (planning range, not validated)", m[0], m[1]))
                .unwrap_or("n/a".into()),
            if p.release_profile {
                "yes"
            } else {
                "no (diagnostic only)"
            },
            if p.allow_warnings { "yes" } else { "no" }
        )
        .replace('@', "@@"),
    );
    f.set_align(Align::Left | Align::Inside | Align::Wrap);
    col.fixed(&f, 56);
    let d = be_diagnostics::descriptors(fixture);
    let mut t = DataTable::new(
        &[
            "Step",
            "Test",
            "Load",
            "Required",
            "Timeout",
            "Requires",
            "Parameters",
        ],
        &[150, 250, 100, 70, 70, 150, 300],
        *th,
    );
    t.set_rows(
        p.steps
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let desc = d.iter().find(|x| x.id == s.test);
                Row::new(
                    i.to_string(),
                    vec![
                        s.key().into(),
                        desc.map(|d| d.name.clone()).unwrap_or(s.test.clone()),
                        desc.map(|d| d.load.label().to_string()).unwrap_or_default(),
                        if s.required {
                            "Yes".into()
                        } else {
                            "Optional".into()
                        },
                        format!("{} s", s.timeout_s),
                        s.requires.join(", "),
                        if s.params.is_null() {
                            String::new()
                        } else {
                            s.params.to_string()
                        },
                    ],
                )
            })
            .collect(),
    );
    if !p.manual_checks.is_empty() {
        let mut m = Frame::default().with_label(
            &format!(
                "Manual checks: {}",
                p.manual_checks
                    .iter()
                    .map(|c| c.label.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
            .replace('@', "@@"),
        );
        m.set_align(Align::Left | Align::Inside | Align::Wrap);
        col.fixed(&m, 70);
    }
    col.end();
    win.end();
    win.make_resizable(true);
    win.show();
}
