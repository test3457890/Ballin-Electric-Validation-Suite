//! Industrial (default), Classic and Dark themes from design/Design Tokens.json.
//! Same layout in every theme; status is always shown as text plus color.

use be_core::model::EventLevel;
use be_core::status::TestStatus;
use fltk::app;
use fltk::enums::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    Industrial,
    Classic,
    Dark,
}

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub kind: ThemeKind,
    pub chrome: Color,
    pub toolbar: Color,
    /// Title / header accent (reserved for title-bar styling where the WM allows).
    #[allow(dead_code)]
    pub accent: Color,
    pub content: Color,
    pub secondary: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub selection: Color,
    pub log_bg: Color,
    pub log_text: Color,
    pub pass: Color,
    pub warn: Color,
    pub fail: Color,
    pub running: Color,
    pub pending: Color,
    pub fail_row: Color,
    pub banner_bg: Color,
    pub banner_fg: Color,
}

const fn hex(v: u32) -> Color {
    Color::from_hex(v)
}

impl Theme {
    pub fn get(kind: ThemeKind) -> Theme {
        match kind {
            ThemeKind::Industrial => Theme {
                kind,
                chrome: hex(0xD5D9DC),
                toolbar: hex(0xE2E5E7),
                accent: hex(0x34434E),
                content: hex(0xFFFFFF),
                secondary: hex(0xF1F3F4),
                border: hex(0x929CA4),
                text: hex(0x202020),
                muted: hex(0x555555),
                selection: hex(0xCBDCEE),
                log_bg: hex(0x181818),
                log_text: hex(0xD8D8D8),
                pass: hex(0x26752A),
                warn: hex(0x946600),
                fail: hex(0xBD1818),
                running: hex(0x245CA2),
                pending: hex(0x666666),
                fail_row: hex(0xFBE3E3),
                banner_bg: hex(0xFFF6D8),
                banner_fg: hex(0x6B4A00),
            },
            ThemeKind::Classic => Theme {
                kind,
                chrome: hex(0xD4D0C8),
                toolbar: hex(0xD4D0C8),
                accent: hex(0x123D79),
                content: hex(0xFFFFFF),
                secondary: hex(0xECE9E2),
                border: hex(0x808080),
                text: hex(0x202020),
                muted: hex(0x505050),
                selection: hex(0xC6D3EA),
                log_bg: hex(0x000000),
                log_text: hex(0xC0C0C0),
                pass: hex(0x1F6B23),
                warn: hex(0x8A5E00),
                fail: hex(0xB01515),
                running: hex(0x123D79),
                pending: hex(0x606060),
                fail_row: hex(0xF7DCDC),
                banner_bg: hex(0xFFFFE1),
                banner_fg: hex(0x5A4000),
            },
            ThemeKind::Dark => Theme {
                kind,
                chrome: hex(0x262A2D),
                toolbar: hex(0x2E3235),
                accent: hex(0x1B2226),
                content: hex(0x202428),
                secondary: hex(0x34393D),
                border: hex(0x555B60),
                text: hex(0xE0E0E0),
                muted: hex(0xA8A8A8),
                selection: hex(0x3A5878),
                log_bg: hex(0x141414),
                log_text: hex(0xD0D0D0),
                pass: hex(0x6CC070),
                warn: hex(0xE0B040),
                fail: hex(0xFF7070),
                running: hex(0x7FB0F0),
                pending: hex(0xA0A0A0),
                fail_row: hex(0x4A2A2A),
                banner_bg: hex(0x3D3420),
                banner_fg: hex(0xF0D890),
            },
        }
    }

    /// Apply global FLTK colors.
    pub fn apply_globals(&self) {
        let (r, g, b) = self.chrome.to_rgb();
        app::background(r, g, b);
        let (r, g, b) = self.content.to_rgb();
        app::background2(r, g, b);
        let (r, g, b) = self.text.to_rgb();
        app::foreground(r, g, b);
        let (r, g, b) = self.selection.to_rgb();
        app::set_selection_color(r, g, b);
    }

    pub fn status_color(&self, s: TestStatus) -> Color {
        match s {
            TestStatus::Pass => self.pass,
            TestStatus::Fail | TestStatus::Aborted => self.fail,
            TestStatus::Warning | TestStatus::Inconclusive => self.warn,
            TestStatus::Running => self.running,
            _ => self.pending,
        }
    }

    /// Log line colors on the dark log background.
    pub fn log_color(&self, l: EventLevel) -> Color {
        match l {
            EventLevel::Error => hex(0xF08080),
            EventLevel::Warn => hex(0xE8C870),
            EventLevel::Pass => hex(0x9CD69C),
            EventLevel::Debug => hex(0x909090),
            EventLevel::Info => self.log_text,
        }
    }
}
