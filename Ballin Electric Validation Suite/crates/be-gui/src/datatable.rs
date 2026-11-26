//! Dense, sortable, row-selectable technical table built on Fl_Table_Row.
//! Click a column header to sort (click again to reverse). Columns are
//! resizable. Keyboard: Up/Down move the selection.

use crate::theme::Theme;
use fltk::enums::{Align, Color, Event, FrameType};
use fltk::prelude::*;
use fltk::table::{TableContext, TableRow, TableRowSelectFlag, TableRowSelectMode};
use fltk::{draw, enums::Font};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Default)]
pub struct Row {
    pub cells: Vec<String>,
    /// Optional text color per cell (status columns).
    pub colors: Vec<Option<Color>>,
    /// Optional row background (e.g. failed rows).
    pub bg: Option<Color>,
    /// Stable key used to keep selection across sorting/updates.
    pub key: String,
}

impl Row {
    pub fn new(key: impl Into<String>, cells: Vec<String>) -> Row {
        let n = cells.len();
        Row {
            cells,
            colors: vec![None; n],
            bg: None,
            key: key.into(),
        }
    }
    pub fn color(mut self, col: usize, c: Color) -> Row {
        if col < self.colors.len() {
            self.colors[col] = Some(c);
        }
        self
    }
    pub fn bg(mut self, c: Color) -> Row {
        self.bg = Some(c);
        self
    }
}

struct Data {
    headers: Vec<String>,
    rows: Vec<Row>,
    sort: Option<(usize, bool)>,
    selected_key: Option<String>,
    theme: Theme,
    font_size: i32,
}

type SelectCallback = Rc<RefCell<Option<Box<dyn FnMut(&str)>>>>;

#[derive(Clone)]
pub struct DataTable {
    pub table: TableRow,
    data: Rc<RefCell<Data>>,
    on_select: SelectCallback,
}

fn sort_key(s: &str) -> (u8, f64, String) {
    // Numeric-aware: leading number sorts numerically; "—"/empty last.
    let t = s.trim();
    if t.is_empty() || t == "—" || t == "-" {
        return (2, 0.0, String::new());
    }
    let num: String = t
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    match num.parse::<f64>() {
        Ok(v) if !num.is_empty() && num != "-" => (0, v, t.to_lowercase()),
        _ => (1, 0.0, t.to_lowercase()),
    }
}

impl DataTable {
    pub fn new(headers: &[&str], widths: &[i32], theme: Theme) -> DataTable {
        let mut t = TableRow::default();
        t.set_type(TableRowSelectMode::Single);
        t.set_cols(headers.len() as i32);
        t.set_col_header(true);
        t.set_col_resize(true);
        t.set_col_resize_min(30);
        t.set_row_header(false);
        t.set_rows(0);
        t.set_frame(FrameType::BorderBox);
        for (i, w) in widths.iter().enumerate() {
            t.set_col_width(i as i32, *w);
        }
        t.end();
        let data = Rc::new(RefCell::new(Data {
            headers: headers.iter().map(|s| s.to_string()).collect(),
            rows: vec![],
            sort: None,
            selected_key: None,
            theme,
            font_size: 13,
        }));
        let on_select: SelectCallback = Rc::new(RefCell::new(None));
        let d = data.clone();
        t.draw_cell(move |t, ctx, row, col, x, y, w, h| {
            let d = d.borrow();
            let th = &d.theme;
            match ctx {
                TableContext::StartPage => draw::set_font(Font::Helvetica, d.font_size),
                TableContext::ColHeader => {
                    draw::push_clip(x, y, w, h);
                    draw::draw_box(FrameType::FlatBox, x, y, w, h, th.toolbar);
                    draw::set_draw_color(th.border);
                    draw::draw_line(x, y + h - 1, x + w, y + h - 1);
                    draw::draw_line(x + w - 1, y, x + w - 1, y + h);
                    draw::set_draw_color(th.text);
                    draw::set_font(Font::Helvetica, d.font_size);
                    let mut label = d.headers.get(col as usize).cloned().unwrap_or_default();
                    if let Some((c, asc)) = d.sort
                        && c == col as usize
                    {
                        label.push_str(if asc { "  ▲" } else { "  ▼" });
                    }
                    draw::draw_text2(&label, x + 6, y, w - 8, h, Align::Left);
                    draw::pop_clip();
                }
                TableContext::Cell => {
                    draw::push_clip(x, y, w, h);
                    let r = d.rows.get(row as usize);
                    let selected = t.row_selected(row);
                    let bg = if selected {
                        th.selection
                    } else if let Some(c) = r.and_then(|r| r.bg) {
                        c
                    } else if row % 2 == 1 {
                        th.secondary
                    } else {
                        th.content
                    };
                    draw::draw_box(FrameType::FlatBox, x, y, w, h, bg);
                    draw::set_draw_color(th.border.lighter());
                    draw::draw_line(x, y + h - 1, x + w, y + h - 1);
                    draw::draw_line(x + w - 1, y, x + w - 1, y + h);
                    if let Some(r) = r {
                        let fg = r
                            .colors
                            .get(col as usize)
                            .copied()
                            .flatten()
                            .unwrap_or(th.text);
                        draw::set_draw_color(fg);
                        draw::set_font(Font::Helvetica, d.font_size);
                        let txt = r.cells.get(col as usize).map(String::as_str).unwrap_or("");
                        draw::draw_text2(txt, x + 6, y, w - 8, h, Align::Left | Align::Clip);
                    }
                    draw::pop_clip();
                }
                _ => {}
            }
        });
        let d2 = data.clone();
        let mut me = DataTable {
            table: t.clone(),
            data,
            on_select,
        };
        let mut me2 = me.clone();
        t.set_callback(move |t| match t.callback_context() {
            TableContext::ColHeader => {
                let col = t.callback_col() as usize;
                {
                    let mut d = d2.borrow_mut();
                    d.sort = match d.sort {
                        Some((c, asc)) if c == col => Some((c, !asc)),
                        _ => Some((col, true)),
                    };
                }
                me2.apply_sort();
            }
            TableContext::Cell => {
                // Fired for mouse clicks and for keyboard navigation (Fl_Table
                // moves its current cell on arrow keys): select that row.
                let row = t.callback_row();
                if row >= 0 && (row as usize) < d2.borrow().rows.len() {
                    let changed = me2.selected_index() != Some(row as usize);
                    me2.select_index(row as usize, changed);
                }
            }
            _ => {}
        });
        me.table.handle(move |t, ev| {
            if ev == Event::Push {
                // Take keyboard focus so arrow keys move the row selection
                // (otherwise they reach the child scrollbar).
                let _ = t.take_focus();
                return false;
            }
            if ev == Event::Focus || ev == Event::Unfocus {
                t.redraw();
                return true;
            }
            false
        });
        me.table.set_row_height_all(22);
        me.table.set_col_header_height(24);
        me
    }

    pub fn set_on_select(&self, f: impl FnMut(&str) + 'static) {
        *self.on_select.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_theme(&mut self, th: Theme) {
        self.data.borrow_mut().theme = th;
        self.table.set_color(th.content);
        self.table.redraw();
    }

    pub fn set_rows(&mut self, rows: Vec<Row>) {
        {
            let mut d = self.data.borrow_mut();
            d.rows = rows;
        }
        self.apply_sort();
    }

    fn apply_sort(&mut self) {
        let n;
        let sel;
        {
            let mut d = self.data.borrow_mut();
            if let Some((c, asc)) = d.sort {
                d.rows.sort_by(|a, b| {
                    let o = sort_key(a.cells.get(c).map(String::as_str).unwrap_or(""))
                        .partial_cmp(&sort_key(b.cells.get(c).map(String::as_str).unwrap_or("")))
                        .unwrap_or(std::cmp::Ordering::Equal);
                    if asc { o } else { o.reverse() }
                });
            }
            n = d.rows.len();
            sel = d
                .selected_key
                .as_ref()
                .and_then(|k| d.rows.iter().position(|r| &r.key == k));
        }
        self.table.set_rows(n as i32);
        self.table.select_all_rows(TableRowSelectFlag::Deselect);
        if let Some(i) = sel {
            let _ = self.table.select_row(i as i32, TableRowSelectFlag::Select);
        }
        self.table.redraw();
    }

    pub fn selected_index(&self) -> Option<usize> {
        let d = self.data.borrow();
        d.selected_key
            .as_ref()
            .and_then(|k| d.rows.iter().position(|r| &r.key == k))
    }

    pub fn select_index(&mut self, i: usize, notify: bool) {
        let key = self.data.borrow().rows.get(i).map(|r| r.key.clone());
        if let Some(k) = key {
            self.data.borrow_mut().selected_key = Some(k.clone());
            self.table.select_all_rows(TableRowSelectFlag::Deselect);
            let _ = self.table.select_row(i as i32, TableRowSelectFlag::Select);
            self.table.redraw();
            if notify && let Some(cb) = self.on_select.borrow_mut().as_mut() {
                cb(&k);
            }
        }
    }

    pub fn select_key(&mut self, key: &str) {
        let i = self.data.borrow().rows.iter().position(|r| r.key == key);
        if let Some(i) = i {
            self.select_index(i, false);
        }
    }

    pub fn row_cells(&self, key: &str) -> Option<Vec<String>> {
        self.data
            .borrow()
            .rows
            .iter()
            .find(|r| r.key == key)
            .map(|r| r.cells.clone())
    }
}
