//! Ballin Electric System Validation Suite — FLTK-rs desktop interface.
//!
//! Traditional industrial layout: menu bar, compact toolbar, system
//! navigator tree, tabbed details with sortable tables, selected-test
//! details, progress and shipment disposition, dark diagnostic event log
//! and status bar. Every reading comes from the engine; nothing is invented.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod chart;
mod datatable;
mod dialogs;
mod theme;

use be_core::capability::Capability;
use be_core::codes;
use be_core::inventory::{Inventory, SensorKind, SensorReading, format_bytes};
use be_core::model::{EventLevel, EventRecord, LoadClass, TestResult};
use be_core::policy::{ReleaseEvaluation, ReleaseState};
use be_core::profile::Profile;
use be_core::status::TestStatus;
use be_core::time::{UtcMillis, format_duration_ms};
use be_engine::{EngineConfig, EngineEvent, StepInfo};
use be_platform::PlatformProvider;
use be_safety::policy::Authorization;
use chart::{Chart, Series};
use datatable::{DataTable, Row};
use fltk::app::{self, Sender};
use fltk::button::Button;
use fltk::enums::{Align, Color, Event, Font, FrameType, Shortcut};
use fltk::frame::Frame;
use fltk::group::{Flex, Group, Tabs, Tile};
use fltk::menu::{Choice, MenuBar, MenuFlag};
use fltk::misc::Progress;
use fltk::prelude::*;
use fltk::text::{StyleTableEntry, TextBuffer, TextDisplay};
use fltk::tree::{Tree, TreeReason, TreeSelect};
use fltk::window::Window;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use theme::{Theme, ThemeKind};

#[derive(Clone)]
pub enum Msg {
    Scan,
    InventoryDone(Box<Inventory>, Vec<Capability>),
    RunProfile,
    RunTargeted,
    Stop,
    Engine(EngineEvent),
    EngineEnded(Option<String>),
    SensorTick,
    SelectProfile(usize),
    ProfileDetails,
    AuthorizeHighLoad,
    RevokeHighLoad,
    ShowResults,
    SaveJson,
    ExportCsv,
    ExportHtml(bool),
    Print,
    Backup,
    SupportBundle,
    NewUnit,
    UseUnitForRuns,
    ClearUnit,
    ManualChecks,
    EvaluateRelease,
    ApproveRelease,
    HoldUnit(bool),
    ServiceRecord,
    RecordBaseline,
    FaultCatalog,
    AuditLog,
    SetOperator,
    Zoom(f32),
    SetTheme(ThemeKind),
    ClearLog,
    CopyLog,
    LogFilter,
    ViewLogForTest,
    About,
    SafetyNotice,
    Quit,
    TreeSelect(String),
    TestRowSelected(String),
    SensorSelected(String),
    UnitSelected(String),
    FaultSelected(String),
}

struct RunState {
    cancel: Arc<AtomicBool>,
    current: Option<usize>,
    progress: Option<f64>,
    phase: String,
    step_started: Instant,
}

struct State {
    theme: Theme,
    operator: String,
    fixture_mode: bool,
    db_path: PathBuf,
    store: Option<be_storage::Store>,
    store_error: Option<String>,
    platform: Box<dyn PlatformProvider>,
    inventory: Option<Inventory>,
    caps: Vec<Capability>,
    profiles: Vec<Profile>,
    profile_idx: usize,
    steps: Vec<StepInfo>,
    results: Vec<Option<TestResult>>,
    run: Option<RunState>,
    last_run_id: Option<i64>,
    last_profile: String,
    evaluation: Option<ReleaseEvaluation>,
    events: Vec<EventRecord>,
    log_min: EventLevel,
    log_test_filter: Option<String>,
    t0: Instant,
    sensor_hist: HashMap<String, VecDeque<(f64, Option<f64>)>>,
    sensor_meta: HashMap<String, SensorReading>,
    selected_sensor: Option<String>,
    selected_unit: Option<String>,
    run_unit: Option<String>,
    selected_step: Option<usize>,
    auth: Authorization,
    tree_props: HashMap<String, (String, Vec<(String, String)>)>,
    error_count: usize,
    quit_pending: bool,
}

#[derive(Clone)]
struct Ui {
    win: Window,
    menu: MenuBar,
    toolbar: Flex,
    btn_scan: Button,
    btn_run: Button,
    btn_stop: Button,
    btn_save: Button,
    btn_export: Button,
    btn_print: Button,
    banner: Frame,
    tree: Tree,
    heading: Frame,
    subheading: Frame,
    tabs: Tabs,
    tab_overview: Flex,
    tab_hardware: Flex,
    tab_sensors: Flex,
    tab_tests: Flex,
    tab_errors: Flex,
    tab_mfg: Flex,
    overview: DataTable,
    caps_table: DataTable,
    hw_title: Frame,
    hw_table: DataTable,
    sensor_table: DataTable,
    chart: Chart,
    profile_choice: Choice,
    unit_label: Frame,
    tests_table: DataTable,
    detail_title: Frame,
    detail_fault: Frame,
    detail_next: Frame,
    detail_cov: Frame,
    btn_viewlog: Button,
    btn_targeted: Button,
    progress_label: Frame,
    progress: Progress,
    disposition: Frame,
    faults_table: DataTable,
    fault_detail: TextBuffer,
    fault_disp: TextDisplay,
    units_table: DataTable,
    unit_detail: TextBuffer,
    unit_disp: TextDisplay,
    log_header: Frame,
    log_filter: Choice,
    log: TextDisplay,
    log_buf: TextBuffer,
    log_style: TextBuffer,
    status_left: Frame,
    status_right: Frame,
    panels: Vec<Group>,
}

fn opt(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "unknown".into())
}

fn tree_label(s: &str) -> String {
    s.replace('/', "\u{2215}").replace('\\', "\u{2216}")
}

fn is_root() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn styled_button(label: &str, tip: &str, th: &Theme) -> Button {
    let mut b = Button::default().with_label(label);
    b.set_frame(FrameType::ThinUpBox);
    b.set_down_frame(FrameType::ThinDownBox);
    b.set_color(if th.kind == ThemeKind::Dark {
        th.secondary
    } else {
        Color::from_hex(0xF5F5F5)
    });
    b.set_label_size(13);
    EXTRA_BUTTONS.with(|v| v.borrow_mut().push(b.clone()));
    b.set_tooltip(tip);
    b
}

thread_local! {
    /// Every styled button, so theme changes reach dialogs-free panels too.
    static PANEL_HEADERS: std::cell::RefCell<Vec<Frame>> = const { std::cell::RefCell::new(Vec::new()) };
    static DETAILS_PANEL: std::cell::RefCell<Option<Flex>> = const { std::cell::RefCell::new(None) };
    static EXTRA_BUTTONS: std::cell::RefCell<Vec<Button>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn panel_header(label: &str, th: &Theme) -> Frame {
    let mut f = Frame::default().with_label(label);
    f.set_frame(FrameType::FlatBox);
    f.set_color(th.toolbar);
    f.set_label_size(12);
    f.set_align(Align::Left | Align::Inside);
    PANEL_HEADERS.with(|v| v.borrow_mut().push(f.clone()));
    f
}

fn text_display(th: &Theme) -> (TextDisplay, TextBuffer) {
    let mut d = TextDisplay::default();
    let b = TextBuffer::default();
    d.set_buffer(b.clone());
    d.set_frame(FrameType::BorderBox);
    d.set_text_size(13);
    d.set_color(th.content);
    d.set_text_color(th.text);
    d.wrap_mode(fltk::text::WrapMode::AtBounds, 0);
    (d, b)
}

fn steel_icon() -> Option<fltk::image::RgbImage> {
    // Original procedurally generated stainless-steel sphere mark.
    let n = 64usize;
    let mut px = vec![0u8; n * n * 4];
    let c = (n as f64 - 1.0) / 2.0;
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = ((x as f64 - c) / c, (y as f64 - c) / c);
            let r2 = dx * dx + dy * dy;
            let i = (y * n + x) * 4;
            if r2 <= 1.0 {
                let hl = ((dx + 0.4).powi(2) + (dy + 0.5).powi(2)).sqrt();
                let shade = (1.0 - hl / 1.6).clamp(0.0, 1.0);
                let rim = 1.0 - r2.sqrt().powi(6) * 0.5;
                let v = (48.0 + 207.0 * shade.powf(1.6) * rim) as u8;
                px[i] = v;
                px[i + 1] = v.saturating_add(4);
                px[i + 2] = v.saturating_add(8);
                px[i + 3] = 255;
            }
        }
    }
    fltk::image::RgbImage::new(&px, n as i32, n as i32, fltk::enums::ColorDepth::Rgba8).ok()
}

fn build_ui(s: &Sender<Msg>, th: &Theme, fixture: bool) -> Ui {
    let (ww, wh) = (1320, 860);
    let mut win = Window::default()
        .with_size(ww, wh)
        .with_label(be_core::PRODUCT_NAME);
    win.size_range(1024, 768, 0, 0);
    win.set_xclass("be-validation");
    if let Some(icon) = steel_icon() {
        win.set_icon(Some(icon));
    }
    let mut root = Flex::default_fill().column();
    root.set_pad(0);

    // ---- Menu bar
    let mut menu = MenuBar::default();
    menu.set_frame(FrameType::FlatBox);
    menu.set_text_size(13);
    root.fixed(&menu, 26);
    let m = |menu: &mut MenuBar, path: &str, sc: Shortcut, msg: Msg| {
        menu.add_emit(path, sc, MenuFlag::Normal, *s, msg);
    };
    m(
        &mut menu,
        "&File/&Save Results (JSON)...",
        Shortcut::Ctrl | 's',
        Msg::SaveJson,
    );
    m(
        &mut menu,
        "&File/Export Results (&CSV)...",
        Shortcut::None,
        Msg::ExportCsv,
    );
    m(
        &mut menu,
        "&File/Export &Report (HTML)...",
        Shortcut::Ctrl | 'e',
        Msg::ExportHtml(false),
    );
    m(
        &mut menu,
        "&File/Export C&ustomer Report (HTML)...",
        Shortcut::None,
        Msg::ExportHtml(true),
    );
    m(
        &mut menu,
        "&File/&Print Report...",
        Shortcut::Ctrl | 'p',
        Msg::Print,
    );
    menu.add_emit(
        "&File/Database &Backup...",
        Shortcut::None,
        MenuFlag::MenuDivider,
        *s,
        Msg::Backup,
    );
    m(&mut menu, "&File/E&xit", Shortcut::Ctrl | 'q', Msg::Quit);
    m(
        &mut menu,
        "S&ystem/&Scan Hardware",
        Shortcut::from_key(fltk::enums::Key::F5),
        Msg::Scan,
    );
    m(
        &mut menu,
        "S&ystem/Set &Operator...",
        Shortcut::None,
        Msg::SetOperator,
    );
    m(
        &mut menu,
        "S&ystem/Create Support &Bundle...",
        Shortcut::None,
        Msg::SupportBundle,
    );
    m(
        &mut menu,
        "&Diagnostics/Run &Selected Test Only (Targeted)",
        Shortcut::Ctrl | 't',
        Msg::RunTargeted,
    );
    m(
        &mut menu,
        "&Diagnostics/&Fault Code Catalog...",
        Shortcut::None,
        Msg::FaultCatalog,
    );
    m(
        &mut menu,
        "&Diagnostics/View &Log for Selected Test",
        Shortcut::None,
        Msg::ViewLogForTest,
    );
    for (i, p) in be_engine::builtin_profiles().iter().enumerate() {
        let flag = if i == 0 {
            MenuFlag::Radio | MenuFlag::Value
        } else {
            MenuFlag::Radio
        };
        menu.add_emit(
            &format!("&Tests/Profile/{}", p.name.replace('/', "\\/")),
            Shortcut::None,
            flag,
            *s,
            Msg::SelectProfile(i),
        );
    }
    m(
        &mut menu,
        "&Tests/&Run Selected Profile",
        Shortcut::from_key(fltk::enums::Key::F9),
        Msg::RunProfile,
    );
    m(
        &mut menu,
        "&Tests/&Stop",
        Shortcut::Shift | fltk::enums::Key::F5,
        Msg::Stop,
    );
    m(
        &mut menu,
        "&Tests/Profile &Details...",
        Shortcut::None,
        Msg::ProfileDetails,
    );
    m(
        &mut menu,
        "&Tests/&Authorize High-Load Testing...",
        Shortcut::None,
        Msg::AuthorizeHighLoad,
    );
    m(
        &mut menu,
        "&Tests/Re&voke High-Load Authorization",
        Shortcut::None,
        Msg::RevokeHighLoad,
    );
    m(
        &mut menu,
        "&Manufacturing/&New Unit Record...",
        Shortcut::Ctrl | 'n',
        Msg::NewUnit,
    );
    m(
        &mut menu,
        "&Manufacturing/&Use Selected Unit for Runs",
        Shortcut::None,
        Msg::UseUnitForRuns,
    );
    menu.add_emit(
        "&Manufacturing/&Clear Unit for Runs",
        Shortcut::None,
        MenuFlag::MenuDivider,
        *s,
        Msg::ClearUnit,
    );
    m(
        &mut menu,
        "&Manufacturing/&Manual Inspection Sign-off...",
        Shortcut::None,
        Msg::ManualChecks,
    );
    m(
        &mut menu,
        "&Manufacturing/&Evaluate Release",
        Shortcut::None,
        Msg::EvaluateRelease,
    );
    m(
        &mut menu,
        "&Manufacturing/&Approve Release...",
        Shortcut::None,
        Msg::ApproveRelease,
    );
    m(
        &mut menu,
        "&Manufacturing/Place on &Hold...",
        Shortcut::None,
        Msg::HoldUnit(false),
    );
    menu.add_emit(
        "&Manufacturing/Send to &Rework...",
        Shortcut::None,
        MenuFlag::MenuDivider,
        *s,
        Msg::HoldUnit(true),
    );
    m(
        &mut menu,
        "&Manufacturing/Add &Service Record...",
        Shortcut::None,
        Msg::ServiceRecord,
    );
    m(
        &mut menu,
        "&Manufacturing/Record Hardware &Baseline",
        Shortcut::None,
        Msg::RecordBaseline,
    );
    m(
        &mut menu,
        "&Reports/Show &Results",
        Shortcut::None,
        Msg::ShowResults,
    );
    m(
        &mut menu,
        "&Reports/Export &Report (HTML)...",
        Shortcut::None,
        Msg::ExportHtml(false),
    );
    m(
        &mut menu,
        "&Reports/Export &Customer Report (HTML)...",
        Shortcut::None,
        Msg::ExportHtml(true),
    );
    m(
        &mut menu,
        "&Reports/Export &JSON...",
        Shortcut::None,
        Msg::SaveJson,
    );
    m(
        &mut menu,
        "&Reports/Export &CSV...",
        Shortcut::None,
        Msg::ExportCsv,
    );
    m(
        &mut menu,
        "T&ools/&Audit Log...",
        Shortcut::None,
        Msg::AuditLog,
    );
    m(
        &mut menu,
        "T&ools/&Copy Event Log",
        Shortcut::None,
        Msg::CopyLog,
    );
    m(
        &mut menu,
        "&View/Zoom &In",
        Shortcut::Ctrl | '=',
        Msg::Zoom(1.1),
    );
    m(
        &mut menu,
        "&View/Zoom &Out",
        Shortcut::Ctrl | '-',
        Msg::Zoom(1.0 / 1.1),
    );
    menu.add_emit(
        "&View/&Actual Size",
        Shortcut::Ctrl | '0',
        MenuFlag::MenuDivider,
        *s,
        Msg::Zoom(0.0),
    );
    menu.add_emit(
        "&View/Theme/&Industrial",
        Shortcut::None,
        MenuFlag::Radio | MenuFlag::Value,
        *s,
        Msg::SetTheme(ThemeKind::Industrial),
    );
    menu.add_emit(
        "&View/Theme/&Classic",
        Shortcut::None,
        MenuFlag::Radio,
        *s,
        Msg::SetTheme(ThemeKind::Classic),
    );
    menu.add_emit(
        "&View/Theme/&Dark",
        Shortcut::None,
        MenuFlag::Radio,
        *s,
        Msg::SetTheme(ThemeKind::Dark),
    );
    m(
        &mut menu,
        "&View/C&lear Event Log View",
        Shortcut::None,
        Msg::ClearLog,
    );
    m(
        &mut menu,
        "&Help/&Safety Notice",
        Shortcut::None,
        Msg::SafetyNotice,
    );
    m(
        &mut menu,
        "&Help/&About BE Validation",
        Shortcut::from_key(fltk::enums::Key::F1),
        Msg::About,
    );

    // ---- Toolbar
    let mut toolbar = Flex::default().row();
    toolbar.set_margin(6);
    toolbar.set_pad(4);
    toolbar.set_frame(FrameType::FlatBox);
    root.fixed(&toolbar, 44);
    let b = |label: &str, tip: &str, w: i32, msg: Msg, tb: &mut Flex| {
        let mut btn = styled_button(label, tip, th);
        btn.emit(*s, msg);
        tb.fixed(&btn, w);
        btn
    };
    let btn_scan = b(
        "@reload  Scan",
        "Re-read hardware inventory (F5). Read-only.",
        92,
        Msg::Scan,
        &mut toolbar,
    );
    let btn_run = b(
        "@>  Run Tests",
        "Run the selected test profile (F9)",
        112,
        Msg::RunProfile,
        &mut toolbar,
    );
    let btn_stop = b(
        "@square  Stop",
        "Stop the running test (Shift+F5)",
        86,
        Msg::Stop,
        &mut toolbar,
    );
    let _btn_results = b(
        "@menu  Results",
        "Show the test results table",
        104,
        Msg::ShowResults,
        &mut toolbar,
    );
    let btn_save = b(
        "@filesave  Save",
        "Save results of the last run as JSON (Ctrl+S)",
        86,
        Msg::SaveJson,
        &mut toolbar,
    );
    let btn_export = b(
        "@>[]  Export",
        "Export the last run report as HTML (Ctrl+E)",
        96,
        Msg::ExportHtml(false),
        &mut toolbar,
    );
    let btn_print = b(
        "@fileprint  Print",
        "Open the printable report of the last run (Ctrl+P)",
        92,
        Msg::Print,
        &mut toolbar,
    );
    Frame::default();
    let mut banner = Frame::default();
    banner.set_frame(FrameType::BorderBox);
    banner.set_label_size(13);
    banner.set_label(if fixture {
        "FIXTURE MODE • SIMULATED RESULTS • NOT VALID FOR RELEASE"
    } else {
        "LIVE • Read-only / low-impact • High load NOT authorized"
    });
    toolbar.fixed(&banner, 430);
    toolbar.end();

    // ---- Main split area (single Tile: navigator | content, over event log)
    let top_y = 70;
    let status_h = 24;
    let tile_h = wh - top_y - status_h;
    let log_h = 190;
    let nav_w = 290;
    let upper_h = tile_h - log_h;
    let mut tile = Tile::new(0, top_y, ww, tile_h, None);
    let mut limiter = Frame::new(200, top_y + 150, ww - 600, tile_h - 300, None);
    limiter.hide();
    tile.resizable(&limiter);

    let nav = Group::new(0, top_y, nav_w, upper_h, None);
    let mut navf = Flex::new(0, top_y, nav_w, upper_h, None).column();
    navf.set_margin(6);
    navf.set_pad(0);
    let nav_header = panel_header("  SYSTEM NAVIGATOR", th);
    navf.fixed(&nav_header, 26);
    let mut tree = Tree::default();
    tree.set_select_mode(TreeSelect::Single);
    tree.set_frame(FrameType::BorderBox);
    tree.set_item_label_size(13);
    tree.set_show_root(true);
    tree.set_root_label("System (scanning...)");
    {
        let s = *s;
        tree.set_callback(move |t| {
            if t.callback_reason() == TreeReason::Selected
                && let Some(item) = t.callback_item()
                && let Ok(p) = t.item_pathname(&item)
            {
                s.send(Msg::TreeSelect(p));
            }
        });
    }
    navf.end();
    nav.end();
    nav.resizable(&navf);

    let (rx, rw) = (nav_w, ww - nav_w);
    let content = Group::new(rx, top_y, rw, upper_h, None);
    let mut cf = Flex::new(rx, top_y, rw, upper_h, None).column();
    cf.set_margin(8);
    cf.set_pad(2);
    let mut heading = Frame::default().with_label("System Validation");
    heading.set_label_size(19);
    heading.set_align(Align::Left | Align::Inside);
    cf.fixed(&heading, 28);
    let mut subheading = Frame::default().with_label("Scanning hardware...");
    subheading.set_label_size(13);
    subheading.set_align(Align::Left | Align::Inside);
    cf.fixed(&subheading, 20);
    let (tx, ty, tw, tht) = (
        rx + 8,
        top_y + 8 + 28 + 2 + 20 + 2,
        rw - 16,
        upper_h - 16 - 52,
    );
    let mut tabs = Tabs::new(tx, ty, tw, tht, None);
    tabs.set_label_size(13);
    tabs.handle_overflow(fltk::group::TabsOverflow::Compress);
    let (cx, cy, cw, ch) = (tx, ty + 26, tw, tht - 26);
    let tab = |label: &str| {
        let mut f = Flex::new(cx, cy, cw, ch, None).with_label(label).column();
        f.set_margin(6);
        f.set_pad(6);
        f.set_label_size(13);
        f
    };

    // Overview
    let mut tab_overview = tab("  Overview  ");
    let overview = DataTable::new(&["Item", "Value"], &[240, 700], *th);
    let cap_hdr = panel_header(
        "  Capabilities of this build on this machine (planned / unsupported items are shown, never hidden)",
        th,
    );
    tab_overview.fixed(&cap_hdr, 22);
    let caps_table = DataTable::new(
        &["Capability", "Area", "State", "Backend", "Reason"],
        &[250, 110, 120, 170, 400],
        *th,
    );
    tab_overview.end();

    // Hardware
    let mut tab_hardware = tab("  Hardware  ");
    let mut hw_title = Frame::default().with_label("Select a device in the System Navigator");
    hw_title.set_align(Align::Left | Align::Inside);
    hw_title.set_label_size(14);
    tab_hardware.fixed(&hw_title, 22);
    let hw_table = DataTable::new(&["Property", "Value"], &[260, 700], *th);
    tab_hardware.end();

    // Sensors
    let mut tab_sensors = tab("  Sensors  ");
    let sensor_table = DataTable::new(
        &[
            "Sensor", "Type", "Current", "Min", "Max", "Average", "Unit", "Source", "Status",
        ],
        &[230, 95, 85, 75, 75, 80, 50, 130, 160],
        *th,
    );
    let chart = Chart::new(*th);
    tab_sensors.fixed(&chart.widget, 220);
    tab_sensors.end();

    // Tests
    let mut tab_tests = tab("  Tests  ");
    let mut row1 = Flex::default().row();
    row1.set_pad(6);
    tab_tests.fixed(&row1, 26);
    let mut pl = Frame::default().with_label("Profile:");
    pl.set_align(Align::Right | Align::Inside);
    row1.fixed(&pl, 54);
    let mut profile_choice = Choice::default();
    profile_choice.set_text_size(13);
    for p in be_engine::builtin_profiles() {
        profile_choice.add_choice(&p.name.replace('/', "\\/"));
    }
    profile_choice.set_value(0);
    {
        let s = *s;
        profile_choice.set_callback(move |c| s.send(Msg::SelectProfile(c.value().max(0) as usize)));
    }
    row1.fixed(&profile_choice, 280);
    let mut pd = styled_button(
        "Details...",
        "Show the selected profile's steps, limits and planned duration",
        th,
    );
    pd.emit(*s, Msg::ProfileDetails);
    row1.fixed(&pd, 80);
    let mut ul = Frame::default().with_label("Unit:");
    ul.set_align(Align::Right | Align::Inside);
    row1.fixed(&ul, 40);
    let mut unit_label =
        Frame::default().with_label("none (diagnostic run, no manufacturing record)");
    unit_label.set_align(Align::Left | Align::Inside);
    row1.end();
    let tests_table = DataTable::new(
        &[
            "Test module",
            "Result",
            "Duration",
            "Device",
            "Code",
            "Load",
            "Required",
        ],
        &[290, 110, 90, 120, 120, 110, 80],
        *th,
    );
    let mut details = Flex::default().row();
    details.set_frame(FrameType::BorderBox);
    details.set_color(th.secondary);
    DETAILS_PANEL.with(|d| *d.borrow_mut() = Some(details.clone()));
    details.set_margin(8);
    tab_tests.fixed(&details, 104);
    let mut dcol = Flex::default().column();
    dcol.set_pad(2);
    let mut detail_title = Frame::default().with_label("Selected test: none");
    detail_title.set_align(Align::Left | Align::Inside);
    detail_title.set_label_size(14);
    let mut detail_fault = Frame::default();
    detail_fault.set_align(Align::Left | Align::Inside | Align::Clip);
    let mut detail_next = Frame::default();
    detail_next.set_align(Align::Left | Align::Inside | Align::Clip);
    let mut detail_cov = Frame::default();
    detail_cov.set_align(Align::Left | Align::Inside | Align::Clip);
    detail_cov.set_label_size(12);
    dcol.end();
    let mut bcol = Flex::default().column();
    bcol.set_pad(6);
    details.fixed(&bcol, 130);
    Frame::default();
    let mut btn_viewlog =
        styled_button("View log", "Filter the event log to the selected test", th);
    btn_viewlog.emit(*s, Msg::ViewLogForTest);
    bcol.fixed(&btn_viewlog, 28);
    let mut btn_targeted = styled_button(
        "Targeted test",
        "Repeat only the selected test as a diagnostic run (Ctrl+T)",
        th,
    );
    btn_targeted.emit(*s, Msg::RunTargeted);
    bcol.fixed(&btn_targeted, 28);
    Frame::default();
    bcol.end();
    details.end();
    let mut prow = Flex::default().row();
    prow.set_pad(8);
    tab_tests.fixed(&prow, 28);
    let mut progress_label = Frame::default().with_label("Overall progress: idle");
    progress_label.set_align(Align::Left | Align::Inside | Align::Clip);
    prow.fixed(&progress_label, 400);
    let mut progress = Progress::default();
    progress.set_minimum(0.0);
    progress.set_maximum(1.0);
    progress.set_value(0.0);
    progress.set_frame(FrameType::BorderBox);
    prow.fixed(&progress, 260);
    Frame::default();
    let mut disposition = Frame::default().with_label("Shipment disposition: no unit selected");
    disposition.set_frame(FrameType::BorderBox);
    prow.fixed(&disposition, 300);
    prow.end();
    tab_tests.end();

    // Errors
    let mut tab_errors = tab("  Errors  ");
    let faults_table = DataTable::new(
        &["Code", "Severity", "Test / step", "Device", "Observation"],
        &[110, 80, 190, 120, 520],
        *th,
    );
    let (fault_disp, fault_detail) = text_display(th);
    tab_errors.fixed(&fault_disp, 170);
    tab_errors.end();

    // Manufacturing
    let mut tab_mfg = tab("  Manufacturing  ");
    let mut mrow = Flex::default().row();
    mrow.set_pad(4);
    tab_mfg.fixed(&mrow, 28);
    for (label, tip, msg, w) in [
        (
            "New Unit...",
            "Create a manufacturing unit record",
            Msg::NewUnit,
            92,
        ),
        (
            "Use for Runs",
            "Associate test runs with the selected unit",
            Msg::UseUnitForRuns,
            96,
        ),
        (
            "Inspection...",
            "Sign off manual inspection checks for the unit's latest run",
            Msg::ManualChecks,
            92,
        ),
        (
            "Evaluate",
            "Evaluate release policy for the unit's latest run",
            Msg::EvaluateRelease,
            74,
        ),
        (
            "Approve Release...",
            "Authorized release approval (only if policy allows)",
            Msg::ApproveRelease,
            124,
        ),
        (
            "Hold...",
            "Place the unit on hold",
            Msg::HoldUnit(false),
            64,
        ),
        (
            "Rework...",
            "Send the unit to rework",
            Msg::HoldUnit(true),
            74,
        ),
        ("Service...", "Add a service record", Msg::ServiceRecord, 76),
        (
            "Baseline",
            "Record current hardware as a preserved baseline",
            Msg::RecordBaseline,
            74,
        ),
    ] {
        let mut bb = styled_button(label, tip, th);
        bb.set_label_size(12);
        bb.emit(*s, msg);
        mrow.fixed(&bb, w);
    }
    Frame::default();
    mrow.end();
    let units_table = DataTable::new(
        &[
            "Serial",
            "Release state",
            "Configuration",
            "Chassis rev.",
            "Created (UTC)",
        ],
        &[170, 140, 330, 100, 190],
        *th,
    );
    let (unit_disp, unit_detail) = text_display(th);
    tab_mfg.fixed(&unit_disp, 190);
    tab_mfg.end();

    tabs.end();
    tabs.set_value(&tab_tests).ok();
    cf.end();
    content.end();
    content.resizable(&cf);

    // Event log
    let logg = Group::new(0, top_y + upper_h, ww, log_h, None);
    let mut lf = Flex::new(0, top_y + upper_h, ww, log_h, None).column();
    lf.set_margin(6);
    lf.set_pad(0);
    let mut lh = Flex::default().row();
    lh.set_frame(FrameType::FlatBox);
    lf.fixed(&lh, 26);
    let log_header = panel_header("  DIAGNOSTIC EVENT LOG", th);
    let mut log_filter = Choice::default();
    for c in [
        "All levels",
        "Warnings and errors",
        "Errors only",
        "Passes only",
    ] {
        log_filter.add_choice(c);
    }
    log_filter.set_value(0);
    log_filter.set_tooltip("Filter the event log view (stored events are never deleted)");
    {
        let s = *s;
        log_filter.set_callback(move |_| s.send(Msg::LogFilter));
    }
    lh.fixed(&log_filter, 170);
    let mut cb = styled_button("Copy", "Copy the visible event log to the clipboard", th);
    cb.emit(*s, Msg::CopyLog);
    lh.fixed(&cb, 60);
    let mut clb = styled_button(
        "Clear view",
        "Clear the on-screen log (stored events are kept)",
        th,
    );
    clb.emit(*s, Msg::ClearLog);
    lh.fixed(&clb, 84);
    lh.end();
    let mut log = TextDisplay::default();
    let log_buf = TextBuffer::default();
    let log_style = TextBuffer::default();
    log.set_buffer(log_buf.clone());
    log.set_frame(FrameType::BorderBox);
    log.set_text_font(Font::Courier);
    log.set_text_size(13);
    lf.end();
    logg.end();
    logg.resizable(&lf);
    tile.end();

    // ---- Status bar
    let mut sb = Flex::default().row();
    sb.set_margin(2);
    sb.set_frame(FrameType::FlatBox);
    root.fixed(&sb, status_h);
    let mut status_left = Frame::default();
    status_left.set_align(Align::Left | Align::Inside);
    status_left.set_label_size(12);
    let mut status_right = Frame::default();
    status_right.set_align(Align::Right | Align::Inside);
    status_right.set_label_size(12);
    sb.fixed(&status_right, 420);
    sb.end();
    root.end();
    win.end();
    win.make_resizable(true);
    {
        let s = *s;
        win.set_callback(move |_| {
            // Ignore Escape (FLTK default would close the window).
            if app::event() == Event::Close {
                s.send(Msg::Quit);
            }
        });
    }
    // Keep the Tile from shrinking panes to nothing.
    let _ = &mut tile;

    let ui = Ui {
        win,
        menu,
        toolbar,
        btn_scan,
        btn_run,
        btn_stop,
        btn_save,
        btn_export,
        btn_print,
        banner,
        tree,
        heading,
        subheading,
        tabs,
        tab_overview,
        tab_hardware,
        tab_sensors,
        tab_tests,
        tab_errors,
        tab_mfg,
        overview,
        caps_table,
        hw_title,
        hw_table,
        sensor_table,
        chart,
        profile_choice,
        unit_label,
        tests_table,
        detail_title,
        detail_fault,
        detail_next,
        detail_cov,
        btn_viewlog,
        btn_targeted,
        progress_label,
        progress,
        disposition,
        faults_table,
        fault_detail,
        fault_disp,
        units_table,
        unit_detail,
        unit_disp,
        log_header,
        log_filter,
        log,
        log_buf,
        log_style,
        status_left,
        status_right,
        panels: vec![nav, content, logg],
    };
    {
        let s = *s;
        ui.tests_table
            .set_on_select(move |k| s.send(Msg::TestRowSelected(k.to_string())));
        let s = s_clone(&s);
        ui.sensor_table
            .set_on_select(move |k| s.send(Msg::SensorSelected(k.to_string())));
        let s2 = s;
        ui.units_table
            .set_on_select(move |k| s2.send(Msg::UnitSelected(k.to_string())));
        let s3 = s;
        ui.faults_table
            .set_on_select(move |k| s3.send(Msg::FaultSelected(k.to_string())));
    }
    ui
}

fn s_clone(s: &Sender<Msg>) -> Sender<Msg> {
    *s
}

fn apply_theme(ui: &mut Ui, th: &Theme) {
    th.apply_globals();
    ui.menu.set_color(th.secondary);
    ui.menu.set_text_color(th.text);
    ui.toolbar.set_color(th.toolbar);
    EXTRA_BUTTONS.with(|v| {
        for b in v.borrow_mut().iter_mut() {
            b.set_color(if th.kind == ThemeKind::Dark {
                th.secondary
            } else {
                Color::from_hex(0xF5F5F5)
            });
            b.set_label_color(th.text);
        }
    });
    PANEL_HEADERS.with(|v| {
        for f in v.borrow_mut().iter_mut() {
            f.set_color(th.toolbar);
            f.set_label_color(th.text);
        }
    });
    ui.tree.set_color(th.content);
    ui.tree.set_item_label_fgcolor(th.text);
    ui.tree.set_selection_color(th.selection);
    ui.heading.set_label_color(th.text);
    ui.subheading.set_label_color(th.muted);
    ui.tabs.set_selection_color(th.content);
    for t in [
        &mut ui.tab_overview,
        &mut ui.tab_hardware,
        &mut ui.tab_sensors,
        &mut ui.tab_tests,
        &mut ui.tab_errors,
        &mut ui.tab_mfg,
    ] {
        t.set_color(th.content);
        t.set_frame(FrameType::FlatBox);
        t.set_label_color(th.text);
    }
    for p in ui.panels.iter_mut() {
        p.set_frame(FrameType::BorderBox);
        p.set_color(th.content);
    }
    ui.panels[2].set_color(th.chrome);
    for tbl in [
        &mut ui.overview,
        &mut ui.caps_table,
        &mut ui.hw_table,
        &mut ui.sensor_table,
        &mut ui.tests_table,
        &mut ui.faults_table,
        &mut ui.units_table,
    ] {
        tbl.set_theme(*th);
    }
    ui.chart.set_theme(*th);
    DETAILS_PANEL.with(|d| {
        if let Some(p) = d.borrow_mut().as_mut() {
            p.set_color(th.secondary);
        }
    });
    for f in [
        &mut ui.detail_title,
        &mut ui.detail_next,
        &mut ui.detail_cov,
        &mut ui.progress_label,
        &mut ui.unit_label,
        &mut ui.hw_title,
    ] {
        f.set_label_color(th.text);
    }
    for d in [&mut ui.fault_disp, &mut ui.unit_disp] {
        d.set_color(th.content);
        d.set_text_color(th.text);
    }
    ui.log.set_color(th.log_bg);
    ui.log.set_text_color(th.log_text);
    let styles: Vec<StyleTableEntry> = [
        EventLevel::Info,
        EventLevel::Pass,
        EventLevel::Warn,
        EventLevel::Error,
        EventLevel::Debug,
    ]
    .iter()
    .map(|l| StyleTableEntry {
        color: th.log_color(*l),
        font: Font::Courier,
        size: 13,
    })
    .collect();
    ui.log.set_highlight_data(ui.log_style.clone(), styles);
    ui.status_left.set_label_color(th.text);
    ui.status_right.set_label_color(th.text);
    ui.win.set_color(th.chrome);
    app::redraw();
}

fn style_char(l: EventLevel) -> char {
    match l {
        EventLevel::Info => 'A',
        EventLevel::Pass => 'B',
        EventLevel::Warn => 'C',
        EventLevel::Error => 'D',
        EventLevel::Debug => 'E',
    }
}

fn log_visible(st: &State, e: &EventRecord) -> bool {
    let lvl_ok = match st.log_min {
        EventLevel::Pass => e.level == EventLevel::Pass,
        m => e.level >= m || (m == EventLevel::Warn && e.level == EventLevel::Error),
    };
    let test_ok = st
        .log_test_filter
        .as_ref()
        .map(|t| e.message.contains(t.as_str()))
        .unwrap_or(true);
    lvl_ok && test_ok
}

fn append_log_line(ui: &mut Ui, e: &EventRecord) {
    let line = format!("{}\n", e.line());
    // Style buffer is byte-indexed (UTF-8): one style byte per text byte.
    let style: String = std::iter::repeat_n(style_char(e.level), line.len()).collect();
    ui.log_buf.append(&line);
    ui.log_style.append(&style);
    let n = ui.log_buf.count_lines(0, ui.log_buf.length());
    ui.log.scroll(n, 0);
}

fn rebuild_log(ui: &mut Ui, st: &State) {
    ui.log_buf.set_text("");
    ui.log_style.set_text("");
    for e in st.events.iter().filter(|e| log_visible(st, e)) {
        append_log_line(ui, e);
    }
}

fn add_event(ui: &mut Ui, st: &mut State, e: EventRecord, persist: bool) {
    if e.level == EventLevel::Error {
        st.error_count += 1;
    }
    if persist && let Some(store) = &st.store {
        let _ = store.add_event(None, &e);
    }
    if log_visible(st, &e) {
        append_log_line(ui, &e);
    }
    st.events.push(e);
    if st.events.len() > 20_000 {
        st.events.drain(0..5_000);
    }
    update_status(ui, st);
}

fn info(ui: &mut Ui, st: &mut State, level: EventLevel, msg: impl Into<String>) {
    add_event(ui, st, EventRecord::new(level, msg), true);
}

fn privilege_label() -> &'static str {
    if is_root() {
        "Administrator (root)"
    } else if cfg!(windows) {
        "Standard (elevation not detected)"
    } else {
        "Standard"
    }
}

fn update_status(ui: &mut Ui, st: &State) {
    let hl = if st.auth.high_load {
        format!(
            "High load: AUTHORIZED ({})",
            st.auth
                .safety_profile
                .as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_default()
        )
    } else {
        "High load: not authorized".into()
    };
    let db = if let Some(e) = &st.store_error {
        format!("DB unavailable: {e}")
    } else {
        format!("DB: {}", st.db_path.display())
    };
    ui.status_left.set_label(&format!(
        "  Offline | Operator: {} | Privilege: {} | {} | {}",
        st.operator,
        privilege_label(),
        hl,
        db
    ));
    let mode = if st.fixture_mode {
        "FIXTURE MODE"
    } else {
        "Live"
    };
    ui.status_right.set_label(&format!(
        "{} Error{} | {} | {} {}  ",
        st.error_count,
        if st.error_count == 1 { "" } else { "s" },
        mode,
        be_core::PRODUCT_SHORT_NAME,
        be_core::ENGINE_VERSION
    ));
    let th = st.theme;
    if st.fixture_mode {
        ui.banner.set_color(th.fail_row);
        ui.banner.set_label_color(th.fail);
    } else if st.auth.high_load {
        ui.banner.set_label(&format!(
            "LIVE • HIGH LOAD AUTHORIZED by {} • profile {}",
            st.auth.operator,
            st.auth
                .safety_profile
                .as_ref()
                .map(|p| p.id.clone())
                .unwrap_or_default()
        ));
        ui.banner.set_color(th.fail_row);
        ui.banner.set_label_color(th.fail);
    } else {
        ui.banner
            .set_label("LIVE • Read-only / low-impact • High load NOT authorized");
        ui.banner.set_color(th.banner_bg);
        ui.banner.set_label_color(th.banner_fg);
    }
    ui.banner.redraw();
    ui.status_left.redraw();
    ui.status_right.redraw();
}

fn update_controls(ui: &mut Ui, st: &State) {
    let running = st.run.is_some();
    let has_run = st.last_run_id.is_some();
    let set = |b: &mut Button, on: bool, tip_on: &str, tip_off: &str| {
        if on {
            b.activate();
            b.set_tooltip(tip_on);
        } else {
            b.deactivate();
            b.set_tooltip(tip_off);
        }
    };
    set(
        &mut ui.btn_run,
        !running,
        "Run the selected test profile (F9)",
        "Unavailable: a test run is in progress",
    );
    set(
        &mut ui.btn_stop,
        running,
        "Stop the running test (Shift+F5)",
        "Unavailable: no test is running",
    );
    set(
        &mut ui.btn_scan,
        !running,
        "Re-read hardware inventory (F5). Read-only.",
        "Unavailable while tests run (avoids skewing results)",
    );
    let no_run_tip = if st.store.is_none() {
        "Unavailable: database not available"
    } else {
        "Unavailable: no stored run yet in this session"
    };
    set(
        &mut ui.btn_save,
        has_run && !running,
        "Save results of the last run as JSON (Ctrl+S)",
        no_run_tip,
    );
    set(
        &mut ui.btn_export,
        has_run && !running,
        "Export the last run report as HTML (Ctrl+E)",
        no_run_tip,
    );
    set(
        &mut ui.btn_print,
        has_run && !running,
        "Open the printable report of the last run (Ctrl+P)",
        no_run_tip,
    );
    let sel = st.selected_step.is_some();
    set(
        &mut ui.btn_targeted,
        sel && !running,
        "Repeat only the selected test as a diagnostic run (Ctrl+T)",
        if running {
            "Unavailable: a run is in progress"
        } else {
            "Unavailable: select a test first"
        },
    );
    set(
        &mut ui.btn_viewlog,
        sel,
        "Filter the event log to the selected test",
        "Unavailable: select a test first",
    );
    if running {
        ui.profile_choice.deactivate();
    } else {
        ui.profile_choice.activate();
    }
    ui.toolbar.redraw();
}

fn fill_tests_table(ui: &mut Ui, st: &State) {
    let th = st.theme;
    let mut rows = Vec::new();
    for (i, step) in st.steps.iter().enumerate() {
        let r = st.results.get(i).and_then(|r| r.as_ref());
        let running = st
            .run
            .as_ref()
            .map(|rs| rs.current == Some(i))
            .unwrap_or(false);
        let status = if running {
            TestStatus::Running
        } else if let Some(r) = r {
            r.status
        } else if st.run.is_some() {
            TestStatus::Pending
        } else {
            TestStatus::NotRun
        };
        let dur = if running {
            format_duration_ms(
                st.run
                    .as_ref()
                    .map(|rs| rs.step_started.elapsed().as_millis() as u64)
                    .unwrap_or(0),
            )
        } else {
            r.filter(|r| r.status.is_terminal() && r.duration_ms > 0)
                .map(|r| format_duration_ms(r.duration_ms))
                .unwrap_or("—".into())
        };
        let code = r
            .and_then(|r| r.first_fault_code().map(String::from))
            .unwrap_or_default();
        let load = step.load.map(|l| l.label().to_string()).unwrap_or_default();
        let mut row = Row::new(
            i.to_string(),
            vec![
                format!("{:02}  {}", i + 1, step.name),
                status.label().to_string(),
                dur,
                step.device.clone(),
                code,
                load,
                if step.required {
                    "Yes".into()
                } else {
                    "Optional".into()
                },
            ],
        )
        .color(1, th.status_color(status));
        if matches!(status, TestStatus::Fail | TestStatus::Aborted) {
            row = row.bg(th.fail_row);
        }
        if step.load == Some(LoadClass::High) {
            row = row.color(5, th.warn);
        }
        rows.push(row);
    }
    ui.tests_table.set_rows(rows);
}

fn steps_from_profile(p: &Profile, fixture: bool) -> Vec<StepInfo> {
    let d = be_diagnostics::descriptors(fixture);
    p.steps
        .iter()
        .map(|s| {
            let desc = d.iter().find(|x| x.id == s.test);
            StepInfo {
                key: s.key().to_string(),
                test_id: s.test.clone(),
                name: desc.map(|d| d.name.clone()).unwrap_or(s.test.clone()),
                required: s.required,
                load: desc.map(|d| d.load),
                device: desc
                    .map(|d| d.subsystem.label().to_string())
                    .unwrap_or_default(),
            }
        })
        .collect()
}

fn show_selected_profile(ui: &mut Ui, st: &mut State) {
    if st.run.is_some() {
        return;
    }
    let p = &st.profiles[st.profile_idx];
    st.steps = steps_from_profile(p, st.fixture_mode);
    st.results = vec![None; st.steps.len()];
    st.selected_step = None;
    ui.progress.set_value(0.0);
    ui.progress_label.set_label(&format!(
        "Overall progress: idle — {} steps in profile",
        st.steps.len()
    ));
    fill_tests_table(ui, st);
    update_detail(ui, st);
    update_heading(ui, st);
}

fn update_heading(ui: &mut Ui, st: &State) {
    let serial = st
        .run_unit
        .clone()
        .unwrap_or_else(|| "no unit record".into());
    let p = &st.profiles[st.profile_idx];
    let sys = st
        .inventory
        .as_ref()
        .map(|i| {
            format!(
                "{} {}",
                i.system.manufacturer.clone().unwrap_or_default(),
                i.system.product.clone().unwrap_or_default()
            )
            .trim()
            .to_string()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "System identity unavailable".into());
    ui.subheading.set_label(&format!(
        "Serial: {} | Profile: {} {} | {} | Station: {}",
        serial,
        p.name,
        p.version,
        sys,
        be_engine::hostname()
    ));
    ui.unit_label.set_label(&match &st.run_unit {
        Some(u) => format!("{u} (results recorded to manufacturing record)"),
        None => "none (diagnostic run, no manufacturing record)".into(),
    });
    let th = st.theme;
    let (txt, bg, fg) = match (&st.evaluation, &st.run_unit) {
        (Some(e), Some(_)) if e.recommended_state == ReleaseState::Hold => (
            "Shipment disposition: HOLD".to_string(),
            th.fail_row,
            th.fail,
        ),
        (Some(e), Some(_)) => (
            format!("Shipment disposition: {}", e.recommended_state.label()),
            th.content,
            if e.eligible_for_review {
                th.pass
            } else {
                th.warn
            },
        ),
        (None, Some(u)) => {
            let stt = st
                .store
                .as_ref()
                .and_then(|s| s.get_unit(u).ok().flatten())
                .map(|u| u.release_state.label().to_string())
                .unwrap_or("unknown".into());
            (format!("Shipment disposition: {stt}"), th.content, th.text)
        }
        _ => (
            "Shipment disposition: not tracked (no unit)".into(),
            th.content,
            th.muted,
        ),
    };
    ui.disposition.set_label(&txt);
    ui.disposition.set_color(bg);
    ui.disposition.set_label_color(fg);
    ui.disposition.redraw();
    ui.subheading.redraw();
    ui.unit_label.redraw();
}

fn update_detail(ui: &mut Ui, st: &State) {
    let th = st.theme;
    let Some(i) = st.selected_step else {
        ui.detail_title
            .set_label("Selected test: none — click a row in the table");
        ui.detail_fault.set_label("");
        ui.detail_next.set_label("");
        ui.detail_cov.set_label("");
        update_controls(ui, st);
        return;
    };
    let step = &st.steps[i];
    ui.detail_title
        .set_label(&format!("Selected test: {} ({})", step.name, step.test_id));
    match st.results.get(i).and_then(|r| r.as_ref()) {
        Some(r) => {
            if let Some(f) = r.faults.first() {
                ui.detail_fault.set_label(
                    &format!("{}: {}. {}", f.code, f.message, f.observation).replace('@', "@@"),
                );
                ui.detail_fault.set_label_color(
                    if r.status == TestStatus::Fail || r.status == TestStatus::Aborted {
                        th.fail
                    } else {
                        th.warn
                    },
                );
                let next = codes::lookup(&f.code)
                    .and_then(|c| c.next_steps.first().copied())
                    .unwrap_or("Review evidence in the event log.");
                ui.detail_next.set_label(&format!(
                    "Next: {next} Root cause is not established by this result alone."
                ));
            } else {
                ui.detail_fault.set_label(
                    &format!(
                        "Result: {}{}",
                        r.status.label(),
                        r.notes
                            .first()
                            .map(|n| format!(" — {n}"))
                            .unwrap_or_default()
                    )
                    .replace('@', "@@"),
                );
                ui.detail_fault.set_label_color(th.status_color(r.status));
                ui.detail_next.set_label(
                    &format!(
                        "Measurements: {}",
                        r.measurements
                            .iter()
                            .take(4)
                            .map(|m| format!("{} = {}", m.name, m.display_value()))
                            .collect::<Vec<_>>()
                            .join("; ")
                    )
                    .replace('@', "@@"),
                );
            }
            ui.detail_cov.set_label(
                &format!(
                    "Coverage: {}",
                    if r.coverage.is_empty() {
                        "—"
                    } else {
                        &r.coverage
                    }
                )
                .replace('@', "@@"),
            );
        }
        None => {
            let d = be_diagnostics::find(&step.test_id, st.fixture_mode).map(|t| t.descriptor());
            ui.detail_fault.set_label(&format!(
                "Not yet run in this session. Load class: {}.",
                step.load.map(|l| l.label()).unwrap_or("?")
            ));
            ui.detail_fault.set_label_color(th.text);
            ui.detail_next.set_label(
                &d.as_ref()
                    .map(|d| d.description.clone())
                    .unwrap_or_default()
                    .replace('@', "@@"),
            );
            ui.detail_cov.set_label(
                &format!(
                    "Limitations: {}",
                    d.map(|d| d.limitations).unwrap_or_default()
                )
                .replace('@', "@@"),
            );
        }
    }
    for f in [
        &mut ui.detail_title,
        &mut ui.detail_fault,
        &mut ui.detail_next,
        &mut ui.detail_cov,
    ] {
        f.redraw();
    }
    update_controls(ui, st);
}

fn fill_faults(ui: &mut Ui, st: &State) {
    let mut rows = Vec::new();
    for (i, r) in st.results.iter().enumerate() {
        let Some(r) = r else { continue };
        for (j, f) in r.faults.iter().enumerate() {
            let sev = codes::lookup(&f.code)
                .map(|c| format!("{:?}", c.severity))
                .unwrap_or("Unknown".into());
            let color = match sev.as_str() {
                "Critical" | "Error" => st.theme.fail,
                "Warning" => st.theme.warn,
                _ => st.theme.text,
            };
            rows.push(
                Row::new(
                    format!("{i}:{j}"),
                    vec![
                        f.code.clone(),
                        sev,
                        format!("{} [{}]", st.steps[i].name, st.steps[i].key),
                        f.device.clone().unwrap_or_default(),
                        format!("{} — {}", f.message, f.observation),
                    ],
                )
                .color(0, color)
                .color(1, color),
            );
        }
    }
    let n = rows.len();
    ui.faults_table.set_rows(rows);
    ui.tab_errors.set_label(&if n > 0 {
        format!("  Errors ({n})  ")
    } else {
        "  Errors  ".into()
    });
    ui.tabs.redraw();
}

fn fill_overview(ui: &mut Ui, st: &State) {
    let mut rows = Vec::new();
    let mut add = |k: &str, v: String| rows.push(Row::new(k.to_string(), vec![k.to_string(), v]));
    if let Some(inv) = &st.inventory {
        add(
            "System",
            format!(
                "{} {}",
                opt(&inv.system.manufacturer),
                inv.system.product.clone().unwrap_or_default()
            ),
        );
        add("Host name", opt(&inv.system.hostname));
        if let Some(v) = &inv.system.virtualized {
            add("Virtualization", v.clone());
        }
        add(
            "Operating system",
            format!("{} — kernel {}", opt(&inv.os.name), opt(&inv.os.kernel)),
        );
        add(
            "Processor",
            inv.cpu
                .as_ref()
                .map(|c| format!("{} ({} logical CPUs)", opt(&c.model), c.logical_cpus))
                .unwrap_or("unavailable".into()),
        );
        add(
            "System memory",
            inv.memory
                .as_ref()
                .map(|m| format_bytes(m.total_bytes))
                .unwrap_or("unavailable".into()),
        );
        add("Graphics devices", format!("{} detected", inv.gpus.len()));
        add("Storage devices", format!("{} detected", inv.storage.len()));
        add(
            "Sensors",
            format!("{} exposed by supported interfaces", inv.sensors.len()),
        );
        add(
            "Unavailable data items",
            format!(
                "{} (see System Navigator ▸ Unavailable Data)",
                inv.unavailable.len()
            ),
        );
        add("Inventory provider", inv.provider.clone());
        add(
            "Inventory collected (UTC)",
            inv.collected_utc
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
        );
    } else {
        add("Inventory", "not yet collected".into());
    }
    add(
        "Last run",
        st.last_run_id
            .map(|i| format!("run {i} — {}", st.last_profile))
            .unwrap_or("none in this session".into()),
    );
    add(
        "Database",
        match &st.store_error {
            Some(e) => format!("UNAVAILABLE: {e}"),
            None => st.db_path.display().to_string(),
        },
    );
    ui.overview.set_rows(rows);
    let th = st.theme;
    let caps: Vec<Row> = st
        .caps
        .iter()
        .map(|c| {
            let col = match c.state.label() {
                "Available" => th.pass,
                "Degraded" | "Blocked" => th.warn,
                "Unsupported" => th.fail,
                _ => th.pending,
            };
            Row::new(
                c.id.clone(),
                vec![
                    c.name.clone(),
                    c.area.clone(),
                    c.state.label().to_string(),
                    c.backend.clone(),
                    c.state.reason().to_string(),
                ],
            )
            .color(2, col)
        })
        .collect();
    ui.caps_table.set_rows(caps);
}

fn build_tree(ui: &mut Ui, st: &mut State) {
    let Some(inv) = st.inventory.clone() else {
        return;
    };
    let mut t = ui.tree.clone();
    t.clear();
    let root = tree_label(&match (&inv.system.product, &inv.system.hostname) {
        (Some(p), _) => p.clone(),
        (None, Some(h)) => format!("This System ({h})"),
        _ => "This System".into(),
    });
    st.tree_props.clear();
    let mut put = |t: &mut Tree, path: &str, title: &str, props: Vec<(String, String)>| {
        t.add(path);
        st.tree_props
            .insert(path.to_string(), (title.to_string(), props));
    };
    let kv = |k: &str, v: String| (k.to_string(), v);
    if let Some(c) = &inv.cpu {
        put(
            &mut t,
            "Processor",
            "Processor",
            vec![
                kv("Model", opt(&c.model)),
                kv("Vendor", opt(&c.vendor)),
                kv(
                    "Packages",
                    c.packages
                        .map(|v| v.to_string())
                        .unwrap_or("unknown".into()),
                ),
                kv(
                    "Physical cores",
                    c.physical_cores
                        .map(|v| v.to_string())
                        .unwrap_or("unknown".into()),
                ),
                kv("Logical CPUs", c.logical_cpus.to_string()),
                kv(
                    "Base clock",
                    c.base_mhz
                        .map(|v| format!("{v:.0} MHz"))
                        .unwrap_or("unavailable".into()),
                ),
                kv(
                    "Max clock",
                    c.max_mhz
                        .map(|v| format!("{v:.0} MHz"))
                        .unwrap_or("unavailable".into()),
                ),
                kv("Microcode", opt(&c.microcode)),
                kv("Instruction set features", c.features.join(" ")),
                kv("Caches (CPU0)", c.cache.join(", ")),
            ],
        );
    } else {
        put(
            &mut t,
            "Processor",
            "Processor",
            vec![kv("Status", "unavailable".into())],
        );
    }
    let mut mem = vec![];
    if let Some(m) = &inv.memory {
        mem.push(kv("Total (OS visible)", format_bytes(m.total_bytes)));
        mem.push(kv(
            "Available",
            m.available_bytes
                .map(format_bytes)
                .unwrap_or("unknown".into()),
        ));
        mem.push(kv(
            "Swap",
            m.swap_total_bytes
                .map(format_bytes)
                .unwrap_or("unknown".into()),
        ));
        for e in &m.ecc_controllers {
            mem.push(kv(
                &format!("{} ECC corrected / uncorrected", e.name),
                format!("{:?} / {:?}", e.corrected, e.uncorrected),
            ));
        }
        if m.ecc_controllers.is_empty() {
            mem.push(kv(
                "ECC counters",
                "unavailable (see Unavailable Data)".into(),
            ));
        }
    }
    put(&mut t, "System Memory", "System Memory", mem);
    put(
        &mut t,
        "Motherboard & Firmware",
        "Motherboard & Firmware",
        vec![
            kv("Manufacturer", opt(&inv.system.manufacturer)),
            kv("Product", opt(&inv.system.product)),
            kv("System serial", opt(&inv.system.serial)),
            kv(
                "Board",
                format!(
                    "{} {}",
                    opt(&inv.system.board_vendor),
                    inv.system.board_name.clone().unwrap_or_default()
                ),
            ),
            kv("Chassis type", opt(&inv.system.chassis_type)),
            kv("BIOS vendor", opt(&inv.firmware.bios_vendor)),
            kv("BIOS version", opt(&inv.firmware.bios_version)),
            kv("BIOS date", opt(&inv.firmware.bios_date)),
            kv("UEFI boot", format!("{:?}", inv.firmware.uefi)),
            kv("Secure Boot", format!("{:?}", inv.firmware.secure_boot)),
            kv("TPM present", format!("{:?}", inv.firmware.tpm_present)),
            kv("TPM version", opt(&inv.firmware.tpm_version)),
        ],
    );
    put(
        &mut t,
        "Graphics Devices",
        "Graphics Devices",
        vec![kv("GPUs detected", inv.gpus.len().to_string())],
    );
    for (i, g) in inv.gpus.iter().enumerate() {
        put(
            &mut t,
            &format!("Graphics Devices/GPU {i}  {}", tree_label(&g.id)),
            &format!("GPU {i}"),
            vec![
                kv("Identifier (PCI address)", g.id.clone()),
                kv("Vendor", opt(&g.vendor)),
                kv("Name", opt(&g.name)),
                kv("Driver", opt(&g.driver)),
                kv(
                    "VRAM total",
                    g.vram_total_bytes
                        .map(format_bytes)
                        .unwrap_or("unavailable".into()),
                ),
                kv(
                    "VRAM used",
                    g.vram_used_bytes
                        .map(format_bytes)
                        .unwrap_or("unavailable".into()),
                ),
                kv(
                    "Telemetry source",
                    g.telemetry_source.clone().unwrap_or("none".into()),
                ),
            ],
        );
    }
    put(
        &mut t,
        "Storage Devices",
        "Storage Devices",
        vec![kv("Devices", inv.storage.len().to_string())],
    );
    for d in &inv.storage {
        put(
            &mut t,
            &format!("Storage Devices/{}", tree_label(&d.name)),
            &d.name,
            vec![
                kv("Model", opt(&d.model)),
                kv("Serial", opt(&d.serial)),
                kv("Firmware", opt(&d.firmware)),
                kv(
                    "Capacity",
                    d.size_bytes.map(format_bytes).unwrap_or("unknown".into()),
                ),
                kv("Transport", opt(&d.transport)),
                kv("Rotational", format!("{:?}", d.rotational)),
                kv("Removable", format!("{:?}", d.removable)),
                kv("State", opt(&d.state)),
            ],
        );
    }
    let sensor_props = |kinds: &[SensorKind]| -> Vec<(String, String)> {
        let v: Vec<_> = inv
            .sensors
            .iter()
            .filter(|s| kinds.contains(&s.kind))
            .map(|s| {
                (
                    format!("{} {}", s.chip, s.label),
                    s.value
                        .map(|v| format!("{v:.2} {}", s.unit))
                        .unwrap_or("unavailable".into()),
                )
            })
            .collect();
        if v.is_empty() {
            vec![(
                "Sensors".into(),
                "none exposed by supported interfaces (see Unavailable Data)".into(),
            )]
        } else {
            v
        }
    };
    put(
        &mut t,
        "Cooling System",
        "Cooling System",
        sensor_props(&[SensorKind::Temperature, SensorKind::Fan]),
    );
    let mut pw = sensor_props(&[SensorKind::Power, SensorKind::Voltage, SensorKind::Current]);
    pw.push((
        "Note".into(),
        "Software telemetry cannot certify PSU ripple, transient response or electrical safety."
            .into(),
    ));
    put(&mut t, "Power Delivery", "Power Delivery", pw);
    put(
        &mut t,
        "Network Devices",
        "Network Devices",
        vec![kv("Interfaces", inv.network.len().to_string())],
    );
    for n in &inv.network {
        put(
            &mut t,
            &format!("Network Devices/{}", tree_label(&n.name)),
            &n.name,
            vec![
                kv("MAC", opt(&n.mac)),
                kv("Driver", opt(&n.driver)),
                kv("State", opt(&n.oper_state)),
                kv(
                    "Link speed",
                    n.speed_mbps
                        .map(|s| format!("{s} Mb/s"))
                        .unwrap_or("unknown".into()),
                ),
                kv(
                    "RX / TX errors",
                    format!("{:?} / {:?}", n.rx_errors, n.tx_errors),
                ),
                kv("Virtual", n.is_virtual.to_string()),
            ],
        );
    }
    put(
        &mut t,
        "External Ports (USB)",
        "External Ports (USB)",
        vec![
            kv("USB devices", inv.usb.len().to_string()),
            kv(
                "Note",
                "Physical connector coverage requires known test devices and operator mapping."
                    .into(),
            ),
        ],
    );
    for u in &inv.usb {
        put(
            &mut t,
            &format!(
                "External Ports (USB)/{} {}",
                tree_label(&u.path),
                tree_label(&u.product.clone().unwrap_or_default())
            ),
            &u.path,
            vec![
                kv(
                    "Vendor:Product",
                    format!("{}:{}", opt(&u.vendor_id), opt(&u.product_id)),
                ),
                kv("Manufacturer", opt(&u.manufacturer)),
                kv("Product", opt(&u.product)),
                kv("Speed", opt(&u.speed)),
            ],
        );
    }
    put(
        &mut t,
        "PCI Devices",
        "PCI Devices",
        vec![kv("Functions", inv.pci.len().to_string())],
    );
    for p in &inv.pci {
        let vendor = p
            .vendor_id
            .map(|v| {
                be_platform::pci_ids::vendor_name(v)
                    .map(String::from)
                    .unwrap_or(format!("0x{v:04x}"))
            })
            .unwrap_or("unknown".into());
        put(
            &mut t,
            &format!(
                "PCI Devices/{} {}",
                tree_label(&p.address),
                tree_label(&p.class_name)
            ),
            &p.address,
            vec![
                kv("Class", p.class_name.clone()),
                kv("Vendor", vendor),
                kv(
                    "Device ID",
                    p.device_id
                        .map(|d| format!("0x{d:04x}"))
                        .unwrap_or("unknown".into()),
                ),
                kv("Driver", opt(&p.driver)),
                kv(
                    "Link (current)",
                    format!(
                        "x{} @ {}",
                        p.current_link_width
                            .map(|w| w.to_string())
                            .unwrap_or("?".into()),
                        opt(&p.current_link_speed)
                    ),
                ),
                kv(
                    "Link (maximum)",
                    format!(
                        "x{} @ {}",
                        p.max_link_width
                            .map(|w| w.to_string())
                            .unwrap_or("?".into()),
                        opt(&p.max_link_speed)
                    ),
                ),
                kv(
                    "AER correctable / nonfatal / fatal",
                    format!(
                        "{:?} / {:?} / {:?}",
                        p.aer_correctable, p.aer_nonfatal, p.aer_fatal
                    ),
                ),
            ],
        );
    }
    let units = st
        .store
        .as_ref()
        .and_then(|s| s.list_units().ok())
        .unwrap_or_default();
    put(
        &mut t,
        "Manufacturing",
        "Manufacturing",
        vec![kv("Unit records", units.len().to_string())],
    );
    for u in units.iter().take(200) {
        put(
            &mut t,
            &format!("Manufacturing/{}", tree_label(&u.serial)),
            &u.serial,
            vec![kv("Release state", u.release_state.label().into())],
        );
    }
    put(
        &mut t,
        "Service Records",
        "Service Records",
        vec![kv(
            "View",
            "Select a unit on the Manufacturing tab to see its service history".into(),
        )],
    );
    put(
        &mut t,
        "Event Logs",
        "Event Logs",
        vec![kv("Events this session", st.events.len().to_string())],
    );
    put(
        &mut t,
        "Unavailable Data",
        "Unavailable Data",
        inv.unavailable
            .iter()
            .map(|u| (u.item.clone(), u.reason.clone()))
            .collect(),
    );
    // clear() recreates the root item, so label it after populating.
    t.set_root_label(&root);
    for p in [
        "Graphics Devices",
        "Storage Devices",
        "Network Devices",
        "External Ports (USB)",
        "PCI Devices",
        "Manufacturing",
    ] {
        let _ = t.close(p, false);
    }
    t.redraw();
}

fn fill_units(ui: &mut Ui, st: &State) {
    let th = st.theme;
    let units = st
        .store
        .as_ref()
        .and_then(|s| s.list_units().ok())
        .unwrap_or_default();
    let rows = units
        .iter()
        .map(|u| {
            let c = match u.release_state {
                ReleaseState::Released => th.pass,
                ReleaseState::Hold | ReleaseState::Rework => th.fail,
                ReleaseState::AwaitingReview => th.running,
                _ => th.text,
            };
            Row::new(
                u.serial.clone(),
                vec![
                    u.serial.clone(),
                    u.release_state.label().into(),
                    u.configuration.clone().unwrap_or_default(),
                    u.chassis_revision.clone().unwrap_or_default(),
                    u.created_utc.to_rfc3339(),
                ],
            )
            .color(1, c)
        })
        .collect();
    ui.units_table.set_rows(rows);
    if let Some(sel) = &st.selected_unit {
        ui.units_table.select_key(sel);
    }
}

fn show_unit_detail(ui: &mut Ui, st: &State, serial: &str) {
    let Some(store) = &st.store else { return };
    let mut t = String::new();
    if let Ok(Some(u)) = store.get_unit(serial) {
        t.push_str(&format!(
            "Unit {}  |  state: {}  |  configuration: {}  |  chassis rev: {}  |  technician: {}\n",
            u.serial,
            u.release_state.label(),
            u.configuration.unwrap_or_default(),
            u.chassis_revision.unwrap_or_default(),
            u.assembly_technician.unwrap_or_default()
        ));
    }
    t.push_str("\nRuns:\n");
    for r in store.list_runs(Some(serial), 20).unwrap_or_default() {
        t.push_str(&format!(
            "  run {:<4} {}@{:<8} {:<11} {:<12} {}{}{}\n",
            r.id,
            r.profile_id,
            r.profile_version,
            r.state,
            r.overall_status.unwrap_or_default(),
            r.started_utc.to_rfc3339(),
            if r.fixture { "  [FIXTURE]" } else { "" },
            r.parent_run_id
                .map(|p| format!("  rework of run {p}"))
                .unwrap_or_default()
        ));
    }
    t.push_str("\nDisposition history:\n");
    for d in store.dispositions(serial).unwrap_or_default() {
        t.push_str(&format!(
            "  {} {:<16} by {:<16} {}\n",
            d.ts.to_rfc3339(),
            d.state.label(),
            d.actor,
            d.reason
        ));
    }
    t.push_str("\nService records:\n");
    for s in store.service_records(serial).unwrap_or_default() {
        t.push_str(&format!(
            "  {} {:<16} {:<14} {} [{}]\n",
            s.ts.to_rfc3339(),
            s.kind,
            s.technician,
            s.description,
            s.parts.join(", ")
        ));
    }
    t.push_str("\nBaselines (preserved):\n");
    for b in store.baselines(serial).unwrap_or_default() {
        t.push_str(&format!(
            "  #{} {} {}\n",
            b.id,
            b.created_utc.to_rfc3339(),
            b.note
        ));
    }
    ui.unit_detail.set_text(&t);
}

fn fill_sensors(ui: &mut Ui, st: &State) {
    let th = st.theme;
    if st.sensor_meta.is_empty() {
        let reason = st
            .inventory
            .as_ref()
            .and_then(|i| {
                i.unavailable
                    .iter()
                    .find(|u| u.item == "Temperature sensors")
                    .map(|u| u.reason.clone())
            })
            .unwrap_or("No sensors exposed by supported interfaces".into());
        ui.sensor_table.set_rows(vec![
            Row::new(
                "none",
                vec![
                    "No sensors available".into(),
                    "—".into(),
                    "unavailable".into(),
                    "—".into(),
                    "—".into(),
                    "—".into(),
                    "".into(),
                    "".into(),
                    reason,
                ],
            )
            .color(2, th.pending),
        ]);
        return;
    }
    let mut ids: Vec<&String> = st.sensor_meta.keys().collect();
    ids.sort();
    let rows = ids
        .into_iter()
        .map(|id| {
            let m = &st.sensor_meta[id];
            let h = st.sensor_hist.get(id);
            let vals: Vec<f64> = h
                .map(|h| h.iter().filter_map(|p| p.1).collect())
                .unwrap_or_default();
            let f = |v: Option<f64>| v.map(|v| format!("{v:.2}")).unwrap_or("unavailable".into());
            let min = vals.iter().cloned().reduce(f64::min);
            let max = vals.iter().cloned().reduce(f64::max);
            let avg = if vals.is_empty() {
                None
            } else {
                Some(vals.iter().sum::<f64>() / vals.len() as f64)
            };
            let (status, col) = match (m.value, m.crit) {
                (None, _) => ("NO READING".to_string(), th.fail),
                (Some(v), Some(c)) if v >= c => ("AT/ABOVE CRITICAL".to_string(), th.fail),
                (Some(_), Some(c)) => (format!("OK (crit {c:.0})"), th.pass),
                _ => ("reading".to_string(), th.text),
            };
            Row::new(
                id.clone(),
                vec![
                    format!("{} {}", m.chip, m.label),
                    m.kind.label().into(),
                    f(m.value),
                    f(min),
                    f(max),
                    f(avg),
                    m.unit.clone(),
                    m.source.clone(),
                    status,
                ],
            )
            .color(8, col)
        })
        .collect();
    ui.sensor_table.set_rows(rows);
}

fn update_chart(ui: &mut Ui, st: &State) {
    let mut s = Series {
        window_s: 300.0,
        ..Default::default()
    };
    match st
        .selected_sensor
        .as_ref()
        .and_then(|id| st.sensor_meta.get(id).map(|m| (id, m)))
    {
        Some((id, m)) => {
            s.label = format!("{} {}", m.chip, m.label);
            s.unit = m.unit.clone();
            s.points = st
                .sensor_hist
                .get(id)
                .map(|h| h.iter().cloned().collect())
                .unwrap_or_default();
            s.empty_reason = "No valid samples (sensor not reporting)".into();
        }
        None => {
            s.empty_reason = if st.sensor_meta.is_empty() {
                "No sensors exposed by supported interfaces on this system".into()
            } else {
                "Select a sensor in the table above to plot it".into()
            };
        }
    }
    ui.chart.set_series(s);
}

fn sensor_tick(ui: &mut Ui, st: &mut State) {
    let now = st.t0.elapsed().as_secs_f64();
    let sample = st.platform.sample_sensors();
    let seen: std::collections::HashSet<String> = sample.iter().map(|s| s.id.clone()).collect();
    for r in sample {
        let h = st.sensor_hist.entry(r.id.clone()).or_default();
        h.push_back((now, r.value));
        while h.len() > 600 {
            h.pop_front();
        }
        st.sensor_meta.insert(r.id.clone(), r);
    }
    // Sensors that disappeared get explicit gaps, not stale values.
    for (id, h) in st.sensor_hist.iter_mut() {
        if !seen.contains(id) {
            h.push_back((now, None));
            if let Some(m) = st.sensor_meta.get_mut(id) {
                m.value = None;
            }
        }
    }
    fill_sensors(ui, st);
    update_chart(ui, st);
    if st.run.is_some() {
        fill_tests_table(ui, st);
    }
}

fn start_run(ui: &mut Ui, st: &mut State, s: &Sender<Msg>, profile: Profile, unit: Option<String>) {
    if st.run.is_some() {
        return;
    }
    let high: Vec<String> = steps_from_profile(&profile, st.fixture_mode)
        .iter()
        .filter(|x| x.load == Some(LoadClass::High))
        .map(|x| x.name.clone())
        .collect();
    if !high.is_empty() {
        let msg = if st.auth.high_load {
            format!(
                "This profile contains HIGH-LOAD steps:\n\n  {}\n\nThey generate sustained heat and power draw under the approved safety profile `{}`.\nStay with the unit. Stop remains available at all times.\n\nStart the run?",
                high.join("\n  "),
                st.auth
                    .safety_profile
                    .as_ref()
                    .map(|p| p.id.clone())
                    .unwrap_or_default()
            )
        } else {
            format!(
                "This profile contains high-load steps that will NOT run because high-load testing is not authorized:\n\n  {}\n\nRequired high-load steps will be recorded as NOT RUN and will block release.\n\nContinue with the remaining steps?",
                high.join("\n  ")
            )
        };
        if dialogs::confirm(&msg, "Start run", "Cancel") != Some(0) {
            return;
        }
    }
    let cancel = Arc::new(AtomicBool::new(false));
    st.run = Some(RunState {
        cancel: cancel.clone(),
        current: None,
        progress: None,
        phase: String::new(),
        step_started: Instant::now(),
    });
    st.evaluation = None;
    st.log_test_filter = None;
    let mut cfg = EngineConfig::new(&st.operator);
    cfg.fixture_mode = st.fixture_mode;
    cfg.auth = st.auth.clone();
    let db = st.db_path.clone();
    let use_db = st.store.is_some();
    let s2 = *s;
    std::thread::spawn(move || {
        let store = if use_db {
            be_engine::open_store(Some(&db)).ok()
        } else {
            None
        };
        let platform = be_platform::native();
        let s3 = s2;
        let r = be_engine::run_profile(
            &cfg,
            &profile,
            unit.as_deref(),
            None,
            platform.as_ref(),
            store.as_ref(),
            &cancel,
            &mut move |e| s3.send(Msg::Engine(e)),
        );
        s2.send(Msg::EngineEnded(r.err()));
    });
    ui.tabs.set_value(&ui.tab_tests).ok();
    update_controls(ui, st);
}

fn handle_engine(ui: &mut Ui, st: &mut State, e: EngineEvent) {
    match e {
        EngineEvent::RunStarted {
            run_id,
            profile,
            steps,
        } => {
            st.results = vec![None; steps.len()];
            st.steps = steps;
            st.last_run_id = run_id;
            st.last_profile = profile;
            st.selected_step = None;
            fill_tests_table(ui, st);
            fill_faults(ui, st);
            update_detail(ui, st);
        }
        EngineEvent::StepStarted { index } => {
            if let Some(r) = st.run.as_mut() {
                r.current = Some(index);
                r.progress = None;
                r.phase = "starting".into();
                r.step_started = Instant::now();
            }
            fill_tests_table(ui, st);
        }
        EngineEvent::StepProgress {
            index,
            progress,
            phase,
        } => {
            if let Some(r) = st.run.as_mut() {
                r.current = Some(index);
                r.progress = progress;
                r.phase = phase;
            }
        }
        EngineEvent::StepFinished { index, result } => {
            if let Some(slot) = st.results.get_mut(index) {
                *slot = Some(*result);
            }
            if let Some(r) = st.run.as_mut() {
                r.current = None;
                r.progress = None;
            }
            fill_tests_table(ui, st);
            fill_faults(ui, st);
            if st.selected_step == Some(index) {
                update_detail(ui, st);
            }
        }
        EngineEvent::Log(ev) => add_event(ui, st, ev, false),
        EngineEvent::RunFinished {
            evaluation,
            overall,
            ..
        } => {
            st.evaluation = evaluation;
            let _ = overall;
        }
    }
    update_progress(ui, st);
}

fn update_progress(ui: &mut Ui, st: &State) {
    let total = st.steps.len().max(1);
    let done = st.results.iter().filter(|r| r.is_some()).count();
    if let Some(r) = &st.run {
        let frac = (done as f64 + r.progress.unwrap_or(0.0)) / total as f64;
        ui.progress.set_value(frac);
        let cur = match (r.current, r.progress) {
            (Some(i), Some(p)) => format!(" — step {}: {:.0}% ({})", i + 1, p * 100.0, r.phase),
            (Some(i), None) => format!(
                " — step {}: {}",
                i + 1,
                if r.phase.is_empty() {
                    "running"
                } else {
                    &r.phase
                }
            ),
            _ => String::new(),
        };
        ui.progress_label.set_label(
            &format!("Overall progress: {done} of {total} steps{cur}").replace('@', "@@"),
        );
        ui.progress.set_selection_color(st.theme.running);
    } else {
        ui.progress.set_value(done as f64 / total as f64);
        let worst = st
            .results
            .iter()
            .flatten()
            .map(|r| r.status)
            .max_by_key(|s| s.severity());
        ui.progress.set_selection_color(match worst {
            Some(TestStatus::Pass) => st.theme.pass,
            Some(w) => st.theme.status_color(w),
            None => st.theme.pending,
        });
        if done > 0 {
            ui.progress_label.set_label(&format!(
                "Run complete: {done} of {total} steps — overall {}",
                worst.map(|w| w.label()).unwrap_or("NOT RUN")
            ));
        }
    }
    ui.progress.redraw();
    ui.progress_label.redraw();
}

fn current_report(st: &State) -> Option<be_reporting::ReportData> {
    let id = st.last_run_id?;
    let store = st.store.as_ref()?;
    be_reporting::ReportData::from_store(store, id).ok()
}

fn require_unit(st: &State) -> Option<String> {
    match &st.selected_unit {
        Some(u) => Some(u.clone()),
        None => {
            dialogs::alert("Select a unit on the Manufacturing tab first.");
            None
        }
    }
}

fn handle(ui: &mut Ui, st: &mut State, s: &Sender<Msg>, msg: Msg) {
    match msg {
        Msg::Scan => {
            if st.run.is_some() {
                return;
            }
            info(
                ui,
                st,
                EventLevel::Info,
                "Hardware scan started (read-only).",
            );
            let s2 = *s;
            std::thread::spawn(move || {
                let p = be_platform::native();
                let inv = p.inventory();
                let caps = be_engine::capabilities(p.as_ref());
                s2.send(Msg::InventoryDone(Box::new(inv), caps));
            });
        }
        Msg::InventoryDone(inv, caps) => {
            let n = inv.unavailable.len();
            st.inventory = Some(*inv);
            st.caps = caps;
            build_tree(ui, st);
            fill_overview(ui, st);
            update_heading(ui, st);
            info(
                ui,
                st,
                EventLevel::Info,
                format!(
                    "Hardware enumeration complete ({} unavailable item{}; see Unavailable Data).",
                    n,
                    if n == 1 { "" } else { "s" }
                ),
            );
        }
        Msg::SelectProfile(i) => {
            if st.run.is_some() || i >= st.profiles.len() {
                return;
            }
            st.profile_idx = i;
            ui.profile_choice.set_value(i as i32);
            if let Some(mut item) = ui.menu.find_item(&format!(
                "&Tests/Profile/{}",
                st.profiles[i].name.replace('/', "\\/")
            )) {
                item.set();
            }
            show_selected_profile(ui, st);
        }
        Msg::ProfileDetails => {
            dialogs::profile_details(&st.profiles[st.profile_idx], st.fixture_mode, &st.theme)
        }
        Msg::RunProfile => {
            let p = st.profiles[st.profile_idx].clone();
            let unit = st.run_unit.clone();
            start_run(ui, st, s, p, unit);
        }
        Msg::RunTargeted => {
            let Some(i) = st.selected_step else { return };
            let step = &st.steps[i];
            let base = st.profiles[st.profile_idx]
                .steps
                .iter()
                .find(|x| x.key() == step.key)
                .cloned();
            let mut ps = base.unwrap_or(be_core::profile::ProfileStep {
                key: None,
                test: step.test_id.clone(),
                required: true,
                params: serde_json::Value::Null,
                timeout_s: 900,
                requires: vec![],
                waivable: false,
            });
            ps.requires.clear();
            let p = Profile {
                schema: 1,
                id: format!("targeted-{}", step.test_id.to_lowercase()),
                version: "1.0.0".into(),
                name: format!("Targeted: {}", step.name),
                description: "Single-test diagnostic run (never release-qualifying)".into(),
                planned_minutes: None,
                allow_warnings: false,
                release_profile: false,
                steps: vec![ps],
                manual_checks: vec![],
            };
            info(
                ui,
                st,
                EventLevel::Info,
                format!("Targeted diagnostic requested: {}", step.name),
            );
            start_run(ui, st, s, p, None);
        }
        Msg::Stop => {
            if let Some(r) = &st.run {
                r.cancel.store(true, Ordering::SeqCst);
                info(
                    ui,
                    st,
                    EventLevel::Warn,
                    "Stop requested by operator — stopping current test and cancelling remaining steps.",
                );
            }
        }
        Msg::Engine(e) => handle_engine(ui, st, e),
        Msg::EngineEnded(err) => {
            st.run = None;
            if st.quit_pending {
                app::quit();
                return;
            }
            if let Some(e) = err {
                info(
                    ui,
                    st,
                    EventLevel::Error,
                    format!("Run could not start: {e}"),
                );
                dialogs::alert(&format!("The run could not start:\n\n{e}"));
            }
            fill_tests_table(ui, st);
            update_progress(ui, st);
            update_heading(ui, st);
            update_detail(ui, st);
            fill_units(ui, st);
            fill_overview(ui, st);
            if let Some(u) = st.selected_unit.clone() {
                show_unit_detail(ui, st, &u);
            }
        }
        Msg::SensorTick => sensor_tick(ui, st),
        Msg::AuthorizeHighLoad => {
            if st.run.is_some() {
                dialogs::alert("Authorization cannot change while a run is in progress.");
                return;
            }
            let Some(path) = dialogs::open_file(
                "Select an APPROVED hardware safety profile (JSON)",
                "*.json",
            ) else {
                return;
            };
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => return dialogs::alert(&format!("Cannot read {}: {e}", path.display())),
            };
            let p = match be_safety::HardwareSafetyProfile::from_json(&text) {
                Ok(p) => p,
                Err(e) => return dialogs::alert(&format!("Invalid safety profile: {e}")),
            };
            if !p.approved || p.approved_by.as_deref().unwrap_or("").trim().is_empty() {
                info(
                    ui,
                    st,
                    EventLevel::Warn,
                    format!(
                        "High-load authorization refused: safety profile `{}` is not approved.",
                        p.id
                    ),
                );
                return dialogs::alert(&format!(
                    "Safety profile `{}` is NOT approved by a named engineer.\nHigh-load testing remains blocked.",
                    p.id
                ));
            }
            let txt = format!(
                "Authorize HIGH-LOAD testing for this session?\n\nOperator: {}\nSafety profile: {} {} (approved by {})\n\nHigh-load tests generate sustained heat and power draw. Only proceed on hardware covered by this profile, with the unit supervised. Preflight checks and the sensor watchdog still apply.",
                st.operator,
                p.id,
                p.version,
                p.approved_by.clone().unwrap_or_default()
            );
            if dialogs::confirm(&txt, "Authorize", "Cancel") == Some(0) {
                st.auth = Authorization {
                    operator: st.operator.clone(),
                    high_load: true,
                    safety_profile: Some(p.clone()),
                };
                if let Some(store) = &st.store {
                    let _ = store.audit(
                        &st.operator,
                        "high_load.authorize",
                        &format!("safety_profile={}@{}", p.id, p.version),
                    );
                }
                info(
                    ui,
                    st,
                    EventLevel::Warn,
                    format!(
                        "High-load testing AUTHORIZED by {} with safety profile {}.",
                        st.operator, p.id
                    ),
                );
            }
        }
        Msg::RevokeHighLoad => {
            st.auth = Authorization {
                operator: st.operator.clone(),
                ..Default::default()
            };
            info(ui, st, EventLevel::Info, "High-load authorization revoked.");
        }
        Msg::ShowResults => {
            ui.tabs.set_value(&ui.tab_tests).ok();
            ui.tabs.redraw();
        }
        Msg::SaveJson | Msg::ExportCsv | Msg::ExportHtml(_) | Msg::Print => {
            let Some(d) = current_report(st) else {
                return dialogs::alert(
                    "No stored run is available yet. Run a profile first (results are stored in the local database).",
                );
            };
            let (content, ext, label) = match &msg {
                Msg::SaveJson => (
                    be_reporting::to_json(&d, be_reporting::Audience::Internal),
                    "json",
                    "JSON results",
                ),
                Msg::ExportCsv => (be_reporting::results_csv(&d), "csv", "CSV results"),
                Msg::ExportHtml(true) => (
                    be_reporting::to_html(&d, be_reporting::Audience::Customer),
                    "html",
                    "customer report",
                ),
                _ => (
                    be_reporting::to_html(&d, be_reporting::Audience::Internal),
                    "html",
                    "report",
                ),
            };
            if matches!(msg, Msg::Print) {
                let dir = std::env::temp_dir().join("be-validation-reports");
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join(format!("run-{}-report.html", d.run.id));
                match std::fs::write(&path, content) {
                    Ok(()) => {
                        let opened = dialogs::open_in_system_viewer(&path);
                        info(
                            ui,
                            st,
                            EventLevel::Info,
                            format!(
                                "Printable report written to {}{}",
                                path.display(),
                                if opened {
                                    "; opened in the system viewer — use its Print command."
                                } else {
                                    " (no system viewer available; open it manually to print)."
                                }
                            ),
                        );
                    }
                    Err(e) => dialogs::alert(&format!("Could not write report: {e}")),
                }
                return;
            }
            let Some(path) = dialogs::save_file(
                &format!("Save {label}"),
                &format!("run-{}.{}", d.run.id, ext),
            ) else {
                return;
            };
            match dialogs::write_atomic(&path, content.as_bytes()) {
                Ok(()) => info(
                    ui,
                    st,
                    EventLevel::Info,
                    format!("Saved {label} for run {} to {}", d.run.id, path.display()),
                ),
                Err(e) => dialogs::alert(&format!("Could not save: {e}")),
            }
        }
        Msg::Backup => {
            let Some(store) = &st.store else {
                return dialogs::alert("Database not available.");
            };
            let Some(path) = dialogs::save_file("Database backup", "be-validation-backup.db")
            else {
                return;
            };
            if path.exists() {
                return dialogs::alert(
                    "Choose a new file name; backups never overwrite existing files.",
                );
            }
            match store.backup_to(&path) {
                Ok(()) => info(
                    ui,
                    st,
                    EventLevel::Info,
                    format!("Database backup written to {}", path.display()),
                ),
                Err(e) => dialogs::alert(&format!("Backup failed: {e}")),
            }
        }
        Msg::SupportBundle => {
            let txt = "Create a support bundle file?\n\nIt contains inventory (with host name, serial numbers and MAC addresses removed), capabilities and recent events.\nIt is saved locally only; nothing is uploaded.";
            if dialogs::confirm(txt, "Create", "Cancel") != Some(0) {
                return;
            }
            let Some(path) =
                dialogs::save_file("Save support bundle", "be-validation-support.json")
            else {
                return;
            };
            let mut inv = st.inventory.clone().unwrap_or_default();
            inv.system.hostname = None;
            inv.system.serial = None;
            inv.storage.iter_mut().for_each(|d| d.serial = None);
            inv.network.iter_mut().for_each(|n| n.mac = None);
            let bundle = serde_json::json!({
                "bundle_schema": 1,
                "generated_utc": UtcMillis::now().to_rfc3339(),
                "engine_version": be_core::ENGINE_VERSION,
                "redaction": "hostname, serial numbers and MAC addresses removed",
                "inventory": inv,
                "capabilities": st.caps,
                "recent_events": st.events.iter().rev().take(500).collect::<Vec<_>>(),
            });
            match dialogs::write_atomic(
                &path,
                serde_json::to_string_pretty(&bundle)
                    .unwrap_or_default()
                    .as_bytes(),
            ) {
                Ok(()) => info(
                    ui,
                    st,
                    EventLevel::Info,
                    format!("Support bundle saved to {} (local only).", path.display()),
                ),
                Err(e) => dialogs::alert(&format!("Could not save: {e}")),
            }
        }
        Msg::NewUnit => {
            let Some(store) = &st.store else {
                return dialogs::alert("Database not available.");
            };
            let Some(u) = dialogs::new_unit(&st.operator) else {
                return;
            };
            match store.create_unit(&u, &st.operator) {
                Ok(()) => {
                    st.selected_unit = Some(u.serial.clone());
                    info(
                        ui,
                        st,
                        EventLevel::Info,
                        format!("Unit record {} created (Awaiting Test).", u.serial),
                    );
                    fill_units(ui, st);
                    show_unit_detail(ui, st, &u.serial);
                    build_tree(ui, st);
                }
                Err(e) => dialogs::alert(&format!("Could not create unit: {e}")),
            }
        }
        Msg::UseUnitForRuns => {
            let Some(u) = require_unit(st) else { return };
            st.run_unit = Some(u.clone());
            st.evaluation = None;
            info(
                ui,
                st,
                EventLevel::Info,
                format!("Runs will be recorded against unit {u}."),
            );
            update_heading(ui, st);
        }
        Msg::ClearUnit => {
            st.run_unit = None;
            st.evaluation = None;
            update_heading(ui, st);
        }
        Msg::ManualChecks => {
            let Some(u) = require_unit(st) else { return };
            let Some(store) = &st.store else { return };
            let Some(run) = store.latest_run_for_unit(&u).ok().flatten() else {
                return dialogs::alert(&format!("Unit {u} has no completed run yet."));
            };
            let Ok(profile) = serde_json::from_str::<Profile>(&run.profile_json) else {
                return;
            };
            if profile.manual_checks.is_empty() {
                return dialogs::alert(&format!(
                    "Profile {} (run {}) defines no manual inspection checks.",
                    profile.id, run.id
                ));
            }
            let existing = store.manual_checks(&u, Some(run.id)).unwrap_or_default();
            if let Some(recs) = dialogs::manual_checks(&profile, &existing, &st.operator, run.id) {
                let mut n = 0;
                for r in recs {
                    match store.add_manual_check(&u, Some(run.id), &r) {
                        Ok(()) => n += 1,
                        Err(e) => dialogs::alert(&format!("Could not record {}: {e}", r.check_id)),
                    }
                }
                info(
                    ui,
                    st,
                    EventLevel::Info,
                    format!(
                        "{n} manual check record(s) signed for unit {u}, run {}.",
                        run.id
                    ),
                );
                show_unit_detail(ui, st, &u);
            }
        }
        Msg::EvaluateRelease => {
            let Some(u) = require_unit(st) else { return };
            let Some(store) = &st.store else { return };
            match be_engine::evaluate_unit(store, &u) {
                Ok((run, e)) => {
                    let mut t = format!(
                        "Unit {u}, latest run {run}:\n\n{}\n",
                        if e.eligible_for_review {
                            "ELIGIBLE for authorized release review."
                        } else {
                            "NOT eligible for release."
                        }
                    );
                    for b in &e.blockers {
                        t.push_str(&format!("\n• {b}"));
                    }
                    for n in &e.notes {
                        t.push_str(&format!("\n  note: {n}"));
                    }
                    dialogs::message(&t);
                }
                Err(e) => dialogs::alert(&e),
            }
        }
        Msg::ApproveRelease => {
            let Some(u) = require_unit(st) else { return };
            let Some(reviewer) =
                dialogs::input("Reviewer identity (required for release approval):", "")
            else {
                return;
            };
            let note = dialogs::input("Approval note (optional):", "").unwrap_or_default();
            let Some(store) = &st.store else { return };
            match be_engine::approve_release(store, &u, &reviewer, &note) {
                Ok(_) => {
                    info(
                        ui,
                        st,
                        EventLevel::Pass,
                        format!(
                            "Unit {u} RELEASED by {reviewer} (recorded locally; no shipping system integration)."
                        ),
                    );
                }
                Err(e) => {
                    info(
                        ui,
                        st,
                        EventLevel::Warn,
                        format!("Release refused for {u}: {}", e.blockers.join("; ")),
                    );
                    dialogs::alert(&format!(
                        "Release REFUSED for {u}:\n\n• {}",
                        e.blockers.join("\n• ")
                    ));
                }
            }
            fill_units(ui, st);
            show_unit_detail(ui, st, &u);
        }
        Msg::HoldUnit(rework) => {
            let Some(u) = require_unit(st) else { return };
            let Some(reason) = dialogs::input(
                if rework {
                    "Reason for rework:"
                } else {
                    "Reason for hold:"
                },
                "",
            ) else {
                return;
            };
            if reason.trim().is_empty() {
                return dialogs::alert("A reason is required.");
            }
            let Some(store) = &st.store else { return };
            let d = be_storage::Disposition {
                unit_serial: u.clone(),
                run_id: None,
                state: if rework {
                    ReleaseState::Rework
                } else {
                    ReleaseState::Hold
                },
                actor: st.operator.clone(),
                reason,
                ts: UtcMillis::now(),
            };
            match store.record_disposition(&d) {
                Ok(()) => info(
                    ui,
                    st,
                    EventLevel::Warn,
                    format!("Unit {u} placed on {}.", d.state.label()),
                ),
                Err(e) => dialogs::alert(&e.to_string()),
            }
            fill_units(ui, st);
            show_unit_detail(ui, st, &u);
        }
        Msg::ServiceRecord => {
            let Some(u) = require_unit(st) else { return };
            let Some((kind, desc, parts)) = dialogs::service_record() else {
                return;
            };
            let Some(store) = &st.store else { return };
            match store.add_service_record(&u, &st.operator, &kind, &desc, &parts) {
                Ok(id) => info(
                    ui,
                    st,
                    EventLevel::Info,
                    format!("Service record {id} added for {u}. Re-test before release."),
                ),
                Err(e) => dialogs::alert(&e.to_string()),
            }
            show_unit_detail(ui, st, &u);
        }
        Msg::RecordBaseline => {
            let Some(u) = require_unit(st) else { return };
            let Some(inv) = &st.inventory else {
                return dialogs::alert("Scan hardware first.");
            };
            let Some(note) = dialogs::input(
                "Baseline note (e.g. factory configuration, after GPU upgrade):",
                "",
            ) else {
                return;
            };
            let Some(store) = &st.store else { return };
            match store.add_baseline(
                &u,
                None,
                &serde_json::to_string(inv).unwrap_or_default(),
                &note,
                &st.operator,
            ) {
                Ok(id) => info(
                    ui,
                    st,
                    EventLevel::Info,
                    format!("Baseline #{id} recorded for {u} (previous baselines preserved)."),
                ),
                Err(e) => dialogs::alert(&e.to_string()),
            }
            show_unit_detail(ui, st, &u);
        }
        Msg::FaultCatalog => dialogs::fault_catalog(&st.theme),
        Msg::AuditLog => {
            let Some(store) = &st.store else {
                return dialogs::alert("Database not available.");
            };
            let text: String = store
                .audit_log(500)
                .unwrap_or_default()
                .iter()
                .map(|a| {
                    format!(
                        "{} {:<16} {:<22} {}\n",
                        a.ts.to_rfc3339(),
                        a.actor,
                        a.action,
                        a.details
                    )
                })
                .collect();
            dialogs::text_window("Audit Log (append-only, newest first)", &text, &st.theme);
        }
        Msg::SetOperator => {
            if let Some(op) = dialogs::input("Operator name:", &st.operator)
                && !op.trim().is_empty()
            {
                st.operator = op.trim().to_string();
                if st.auth.high_load {
                    st.auth = Authorization {
                        operator: st.operator.clone(),
                        ..Default::default()
                    };
                    info(
                        ui,
                        st,
                        EventLevel::Warn,
                        "Operator changed: high-load authorization revoked.",
                    );
                }
                info(
                    ui,
                    st,
                    EventLevel::Info,
                    format!("Operator set to {}", st.operator),
                );
            }
        }
        Msg::Zoom(f) => {
            let cur = app::screen_scale(0);
            let next = if f == 0.0 {
                1.0
            } else {
                (cur * f).clamp(0.6, 3.0)
            };
            for i in 0..app::screen_count() {
                app::set_screen_scale(i, next);
            }
        }
        Msg::SetTheme(k) => {
            st.theme = Theme::get(k);
            apply_theme(ui, &st.theme);
            fill_tests_table(ui, st);
            fill_overview(ui, st);
            fill_units(ui, st);
            fill_faults(ui, st);
            rebuild_log(ui, st);
            update_status(ui, st);
            update_heading(ui, st);
        }
        Msg::ClearLog => {
            ui.log_buf.set_text("");
            ui.log_style.set_text("");
            st.events.clear();
        }
        Msg::CopyLog => {
            let start = ui.log_buf.selection_position();
            let text = match start {
                Some((a, b)) if b > a => ui.log_buf.text_range(a, b).unwrap_or_default(),
                _ => ui.log_buf.text(),
            };
            app::copy(&text);
            info(
                ui,
                st,
                EventLevel::Debug,
                format!(
                    "Copied {} characters of the event log to the clipboard.",
                    text.len()
                ),
            );
        }
        Msg::LogFilter => {
            st.log_min = match ui.log_filter.value() {
                1 => EventLevel::Warn,
                2 => EventLevel::Error,
                3 => EventLevel::Pass,
                _ => EventLevel::Debug,
            };
            rebuild_log(ui, st);
        }
        Msg::ViewLogForTest => {
            if let Some(i) = st.selected_step {
                st.log_test_filter =
                    if st.log_test_filter.as_deref() == Some(st.steps[i].name.as_str()) {
                        None
                    } else {
                        Some(st.steps[i].name.clone())
                    };
                ui.log_header.set_label(&match &st.log_test_filter {
                    Some(t) => format!(
                        "  DIAGNOSTIC EVENT LOG — filtered: {t} (click View log again to clear)"
                    ),
                    None => "  DIAGNOSTIC EVENT LOG".into(),
                });
                rebuild_log(ui, st);
            }
        }
        Msg::About => dialogs::message(&format!(
            "{}\n{}\nVersion {} (engine {}, result schema {})\n\nOriginal software by Ballin Electric. Not affiliated with any hardware or software vendor.\nLicense: proprietary — no license has been selected or granted.\nThird-party components: see docs/Third Party Notices.md.\n\nThis software does not replace electrical safety certification, regulatory testing or calibrated instruments.",
            be_core::PRODUCT_NAME,
            be_core::PRODUCT_SUBTITLE,
            env!("CARGO_PKG_VERSION"),
            be_core::ENGINE_VERSION,
            be_core::RESULT_SCHEMA_VERSION
        )),
        Msg::SafetyNotice => dialogs::message(
            "Safety\n\n• Inventory and diagnostics are read-only by default. Low-impact tests use bounded buffers and short durations.\n• High-load (stress / burn-in) tests never start automatically. They require explicit operator authorization AND an approved, device-specific hardware safety profile; preflight checks and a sensor watchdog abort on critical temperature or sensor loss.\n• No firmware changes, overclocking, voltage changes, raw disk writes or destructive tests are performed.\n• Software telemetry cannot certify PSU ripple, grounding, mains safety or regulatory compliance.\n• Stop (Shift+F5) is available at all times during a run.",
        ),
        Msg::Quit => {
            if st.run.is_some() {
                if dialogs::confirm(
                    "A test run is in progress. Stop it and exit?\n\nThe run will be recorded as aborted.",
                    "Stop and exit",
                    "Keep running",
                ) != Some(0)
                {
                    return;
                }
                if let Some(r) = &st.run {
                    r.cancel.store(true, Ordering::SeqCst);
                }
                // Exit once the engine reports the run ended (worker
                // terminated, results persisted); fall back after 8 s.
                st.quit_pending = true;
                app::add_timeout3(8.0, |_| app::quit());
                return;
            }
            app::quit();
        }
        Msg::TreeSelect(path) => {
            let root = ui.tree.root().and_then(|r| r.label()).unwrap_or_default();
            let p = path
                .strip_prefix(&format!("{root}/"))
                .unwrap_or(&path)
                .to_string();
            if p == "Event Logs" {
                let _ = ui.log.take_focus();
                return;
            }
            if let Some(serial) = p.strip_prefix("Manufacturing/") {
                let serial = serial.replace('\u{2215}', "/");
                st.selected_unit = Some(serial.clone());
                ui.tabs.set_value(&ui.tab_mfg).ok();
                fill_units(ui, st);
                show_unit_detail(ui, st, &serial);
                ui.tabs.redraw();
                return;
            }
            if p == "Manufacturing" || p == "Service Records" {
                ui.tabs.set_value(&ui.tab_mfg).ok();
                ui.tabs.redraw();
                return;
            }
            if let Some((title, props)) = st.tree_props.get(&p).cloned() {
                ui.hw_title.set_label(&title.replace('@', "@@"));
                ui.hw_table.set_rows(
                    props
                        .iter()
                        .enumerate()
                        .map(|(i, (k, v))| Row::new(i.to_string(), vec![k.clone(), v.clone()]))
                        .collect(),
                );
                ui.tabs.set_value(&ui.tab_hardware).ok();
                ui.tabs.redraw();
            } else if p == root || path == root {
                ui.tabs.set_value(&ui.tab_overview).ok();
                ui.tabs.redraw();
            }
        }
        Msg::TestRowSelected(k) => {
            st.selected_step = k.parse().ok();
            update_detail(ui, st);
        }
        Msg::SensorSelected(k) => {
            st.selected_sensor = Some(k);
            update_chart(ui, st);
        }
        Msg::UnitSelected(k) => {
            st.selected_unit = Some(k.clone());
            show_unit_detail(ui, st, &k);
        }
        Msg::FaultSelected(k) => {
            let Some(cells) = ui.faults_table.row_cells(&k) else {
                return;
            };
            let code = &cells[0];
            let mut t = format!("{}  —  {}\n", code, cells[4]);
            match codes::lookup(code) {
                Some(c) => {
                    t.push_str(&format!("\nCatalog ({}) — {}: {}\n\nPossible causes (hypotheses, not a diagnosis):\n", codes::CATALOG_VERSION, c.title, c.meaning));
                    for p in c.possible_causes {
                        t.push_str(&format!("  • {p}\n"));
                    }
                    t.push_str("\nSuggested next safe steps:\n");
                    for n in c.next_steps {
                        t.push_str(&format!("  • {n}\n"));
                    }
                }
                None => t.push_str("\nCode not in catalog."),
            }
            ui.fault_detail.set_text(&t);
        }
    }
    update_controls(ui, st);
    update_status(ui, st);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fixture_mode = args.iter().any(|a| a == "--fixture-mode");
    let db_arg = args
        .iter()
        .position(|a| a == "--db")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    let app = app::App::default().with_scheme(app::Scheme::Base);
    app::set_visible_focus(true);
    app::set_font_size(13);
    let theme = Theme::get(ThemeKind::Industrial);
    theme.apply_globals();
    let (s, r) = app::channel::<Msg>();
    let mut ui = build_ui(&s, &theme, fixture_mode);
    apply_theme(&mut ui, &theme);

    let db_path = db_arg.unwrap_or_else(be_storage::default_db_path);
    let (store, store_error) = match be_storage::Store::open(&db_path) {
        Ok(st) => (Some(st), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let interrupted = store
        .as_ref()
        .and_then(|s| s.recover_interrupted().ok())
        .unwrap_or_default();
    let operator = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "Local".into());
    let profiles = be_engine::builtin_profiles();
    let mut st = State {
        theme,
        operator: operator.clone(),
        fixture_mode,
        db_path,
        store,
        store_error,
        platform: be_platform::native(),
        inventory: None,
        caps: vec![],
        profiles,
        profile_idx: 0,
        steps: vec![],
        results: vec![],
        run: None,
        last_run_id: None,
        last_profile: String::new(),
        evaluation: None,
        events: vec![],
        log_min: EventLevel::Debug,
        log_test_filter: None,
        t0: Instant::now(),
        sensor_hist: HashMap::new(),
        sensor_meta: HashMap::new(),
        selected_sensor: None,
        selected_unit: None,
        run_unit: None,
        selected_step: None,
        auth: Authorization {
            operator,
            ..Default::default()
        },
        tree_props: HashMap::new(),
        error_count: 0,
        quit_pending: false,
    };
    ui.win.show();
    info(
        &mut ui,
        &mut st,
        EventLevel::Info,
        format!(
            "{} {} started. Offline operation; no network traffic is generated.",
            be_core::PRODUCT_SHORT_NAME,
            be_core::ENGINE_VERSION
        ),
    );
    if fixture_mode {
        info(
            &mut ui,
            &mut st,
            EventLevel::Warn,
            "FIXTURE MODE enabled: simulated tests are available and every result is labelled as a fixture (never valid for release).",
        );
    }
    if let Some(e) = st.store_error.clone() {
        info(
            &mut ui,
            &mut st,
            EventLevel::Error,
            format!("Database unavailable ({e}); results will not be stored."),
        );
    }
    for id in interrupted {
        add_event(&mut ui, &mut st, EventRecord::new(EventLevel::Error, format!("Run {id} did not complete (crash, power loss or forced exit); marked interrupted, never passed.")).with_code("BE-PWR-001"), false);
    }
    show_selected_profile(&mut ui, &mut st);
    fill_units(&mut ui, &st);
    fill_sensors(&mut ui, &st);
    update_chart(&mut ui, &st);
    update_controls(&mut ui, &st);
    update_status(&mut ui, &st);
    s.send(Msg::Scan);
    {
        app::add_timeout3(1.0, move |h| {
            s.send(Msg::SensorTick);
            app::repeat_timeout3(1.0, h);
        });
    }
    // Optional automatic profile run (GUI smoke tests); same engine path as F9.
    if let Some(i) = args.iter().position(|a| a == "--autorun")
        && let Some(idx) = args
            .get(i + 1)
            .and_then(|id| st.profiles.iter().position(|p| &p.id == id))
    {
        s.send(Msg::SelectProfile(idx));
        s.send(Msg::RunProfile);
    }
    if let Some(i) = args.iter().position(|a| a == "--theme") {
        match args.get(i + 1).map(String::as_str) {
            Some("dark") => s.send(Msg::SetTheme(ThemeKind::Dark)),
            Some("classic") => s.send(Msg::SetTheme(ThemeKind::Classic)),
            _ => {}
        }
    }
    // Optional automatic exit (automated GUI smoke tests under Xvfb).
    if let Some(i) = args.iter().position(|a| a == "--exit-after") {
        let secs: f64 = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(5.0);
        app::add_timeout3(secs, move |_| s.send(Msg::Quit));
    }
    while app.wait() {
        if let Some(msg) = r.recv() {
            handle(&mut ui, &mut st, &s, msg);
        }
    }
}
