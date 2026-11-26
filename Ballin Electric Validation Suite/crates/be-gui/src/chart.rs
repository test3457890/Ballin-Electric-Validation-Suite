//! Instrument-style time-series chart: axes, units, grid, legend, flat trace.
//! Missing samples are drawn as gaps, never as zero.

use crate::theme::Theme;
use fltk::draw;
use fltk::draw::LineStyle;
use fltk::enums::{Align, Font, FrameType};
use fltk::prelude::*;
use fltk::widget::Widget;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default, Clone)]
pub struct Series {
    pub label: String,
    pub unit: String,
    /// (seconds since start of history, value)
    pub points: Vec<(f64, Option<f64>)>,
    pub window_s: f64,
    pub empty_reason: String,
}

#[derive(Clone)]
pub struct Chart {
    pub widget: Widget,
    data: Rc<RefCell<(Series, Theme)>>,
}

fn nice_step(range: f64) -> f64 {
    let raw = range / 5.0;
    let mag = 10f64.powf(raw.log10().floor());
    let n = raw / mag;
    let s = if n < 1.5 {
        1.0
    } else if n < 3.5 {
        2.0
    } else if n < 7.5 {
        5.0
    } else {
        10.0
    };
    s * mag
}

impl Chart {
    pub fn new(theme: Theme) -> Chart {
        let mut w = Widget::default();
        w.set_frame(FrameType::BorderBox);
        let data = Rc::new(RefCell::new((
            Series {
                window_s: 300.0,
                ..Default::default()
            },
            theme,
        )));
        let d = data.clone();
        w.draw(move |w| {
            let (s, th) = &*d.borrow();
            let (x, y, ww, hh) = (w.x(), w.y(), w.w(), w.h());
            draw::draw_box(FrameType::FlatBox, x, y, ww, hh, th.content);
            draw::set_draw_color(th.border);
            draw::draw_rect(x, y, ww, hh);
            draw::set_font(Font::Helvetica, 11);
            let (l, r, t, b) = (x + 58, x + ww - 12, y + 22, y + hh - 26);
            if r - l < 40 || b - t < 30 {
                return;
            }
            // Legend / title.
            draw::set_draw_color(th.text);
            let title = if s.label.is_empty() {
                "Select a sensor in the table above".to_string()
            } else {
                format!("{} [{}]", s.label, s.unit)
            };
            draw::draw_text2(&title, l, y + 3, r - l, 16, Align::Left);
            let vals: Vec<f64> = s.points.iter().filter_map(|p| p.1).collect();
            if vals.is_empty() {
                draw::set_draw_color(th.muted);
                let msg = if s.empty_reason.is_empty() {
                    "No samples"
                } else {
                    &s.empty_reason
                };
                draw::draw_text2(msg, l, t, r - l, b - t, Align::Center | Align::Wrap);
                draw::set_draw_color(th.border);
                draw::draw_rect(l, t, r - l, b - t);
                return;
            }
            let (mut lo, mut hi) = vals
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                    (a.min(*v), b.max(*v))
                });
            if (hi - lo).abs() < 1e-9 {
                lo -= 1.0;
                hi += 1.0;
            }
            let step = nice_step(hi - lo);
            lo = (lo / step).floor() * step;
            hi = (hi / step).ceil() * step;
            let now = s.points.last().map(|p| p.0).unwrap_or(0.0);
            let t0 = now - s.window_s;
            let px = |ts: f64| l as f64 + (ts - t0) / s.window_s * (r - l) as f64;
            let py = |v: f64| b as f64 - (v - lo) / (hi - lo) * (b - t) as f64;
            // Grid and Y axis labels.
            let mut v = lo;
            while v <= hi + step * 0.5 {
                let yy = py(v) as i32;
                draw::set_draw_color(th.secondary.darker());
                draw::set_line_style(LineStyle::Dot, 1);
                draw::draw_line(l, yy, r, yy);
                draw::set_line_style(LineStyle::Solid, 1);
                draw::set_draw_color(th.text);
                draw::draw_text2(&format!("{v:.1}"), x + 2, yy - 7, 52, 14, Align::Right);
                v += step;
            }
            // X axis: seconds ago.
            for k in 0..=5 {
                let ago = s.window_s * (5 - k) as f64 / 5.0;
                let xx = px(now - ago) as i32;
                draw::set_draw_color(th.secondary.darker());
                draw::set_line_style(LineStyle::Dot, 1);
                draw::draw_line(xx, t, xx, b);
                draw::set_line_style(LineStyle::Solid, 1);
                draw::set_draw_color(th.text);
                let lbl = if ago == 0.0 {
                    "now".to_string()
                } else {
                    format!("-{:.0} s", ago)
                };
                draw::draw_text2(&lbl, xx - 30, b + 4, 60, 14, Align::Center);
            }
            draw::set_draw_color(th.border);
            draw::draw_rect(l, t, r - l, b - t);
            draw::set_draw_color(th.running);
            draw::set_line_style(LineStyle::Solid, 2);
            let mut prev: Option<(i32, i32)> = None;
            for (ts, val) in &s.points {
                if *ts < t0 {
                    prev = None;
                    continue;
                }
                match val {
                    Some(v) => {
                        let p = (px(*ts) as i32, py(*v) as i32);
                        if let Some(q) = prev {
                            draw::draw_line(q.0, q.1, p.0, p.1);
                        } else {
                            draw::draw_point(p.0, p.1);
                        }
                        prev = Some(p);
                    }
                    None => prev = None,
                }
            }
            draw::set_line_style(LineStyle::Solid, 1);
            let last = s.points.iter().rev().find_map(|p| p.1);
            draw::set_draw_color(th.text);
            if let Some(lv) = last {
                draw::draw_text2(
                    &format!(
                        "current {lv:.2} {}   min {:.2}   max {:.2}",
                        s.unit,
                        vals.iter().cloned().fold(f64::INFINITY, f64::min),
                        vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
                    ),
                    l,
                    y + 3,
                    r - l,
                    16,
                    Align::Right,
                );
            }
        });
        Chart { widget: w, data }
    }

    pub fn set_series(&mut self, s: Series) {
        self.data.borrow_mut().0 = s;
        self.widget.redraw();
    }

    pub fn set_theme(&mut self, th: Theme) {
        self.data.borrow_mut().1 = th;
        self.widget.redraw();
    }
}
