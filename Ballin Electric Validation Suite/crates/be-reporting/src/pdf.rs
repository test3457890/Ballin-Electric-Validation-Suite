//! Minimal native PDF writer for validation reports (no external crates).
//!
//! PDF 1.4, A4, standard Helvetica / Helvetica-Bold / Courier fonts with
//! WinAnsiEncoding (no font embedding). Text is wrapped by an estimated
//! glyph width and paginated. Characters outside WinAnsi are replaced.

use crate::{Audience, ReportData};
use be_core::codes;
use be_core::inventory::format_bytes;
use be_core::model::DataQuality;
use be_core::status::TestStatus;
use be_core::time::format_duration_ms;
use std::fmt::Write;

const W: f32 = 595.0;
const H: f32 = 842.0;
const MARGIN: f32 = 42.0;

#[derive(Clone, Copy, PartialEq)]
enum Font {
    Regular,
    Bold,
    Mono,
}

impl Font {
    fn res(self) -> &'static str {
        match self {
            Font::Regular => "F1",
            Font::Bold => "F2",
            Font::Mono => "F3",
        }
    }
    /// Approximate average advance width as a fraction of font size.
    fn avg(self) -> f32 {
        match self {
            Font::Mono => 0.60,
            _ => 0.52,
        }
    }
}

struct Doc {
    pages: Vec<String>,
    cur: String,
    y: f32,
    page_no: usize,
    footer: String,
}

/// Map a Unicode string to WinAnsi bytes, escaping PDF string syntax.
fn pdf_text(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            '—' | '–' => out.push('-'),
            '°' => out.push_str("\\260"),
            '•' => out.push_str("\\225"),
            '±' => out.push_str("\\261"),
            'µ' => out.push_str("\\265"),
            '…' => out.push_str("..."),
            '▸' | '→' => out.push('>'),
            '\t' => out.push(' '),
            _ => out.push('?'),
        }
    }
    out
}

fn wrap(s: &str, font: Font, size: f32, width: f32) -> Vec<String> {
    let max = ((width / (size * font.avg())) as usize).max(10);
    let mut lines = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let mut word = word.to_string();
            while word.chars().count() > max {
                let head: String = word.chars().take(max).collect();
                if !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                }
                lines.push(head);
                word = word.chars().skip(max).collect();
            }
            if line.chars().count() + word.chars().count() + 1 > max && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&word);
        }
        lines.push(line);
    }
    lines
}

impl Doc {
    fn new(footer: String) -> Doc {
        Doc {
            pages: vec![],
            cur: String::new(),
            y: H - MARGIN,
            page_no: 1,
            footer,
        }
    }
    fn finish_page(&mut self) {
        let mut c = std::mem::take(&mut self.cur);
        let _ = write!(
            c,
            "BT /F1 7 Tf {MARGIN} 24 Td ({}) Tj ET\nBT /F1 7 Tf {} 24 Td (Page {}) Tj ET\n",
            pdf_text(&self.footer),
            W - MARGIN - 40.0,
            self.page_no
        );
        self.pages.push(c);
        self.page_no += 1;
        self.y = H - MARGIN;
    }
    fn need(&mut self, h: f32) {
        if self.y - h < 44.0 {
            self.finish_page();
        }
    }
    fn text(&mut self, x: f32, s: &str, font: Font, size: f32, gray: f32) {
        let _ = writeln!(
            self.cur,
            "{gray} g BT /{} {size} Tf {x} {} Td ({}) Tj ET 0 g",
            font.res(),
            self.y,
            pdf_text(s)
        );
    }
    fn para(&mut self, s: &str, font: Font, size: f32) {
        for l in wrap(s, font, size, W - 2.0 * MARGIN) {
            self.need(size + 3.0);
            self.text(MARGIN, &l, font, size, 0.0);
            self.y -= size + 3.0;
        }
    }
    fn heading(&mut self, s: &str) {
        self.need(30.0);
        self.y -= 8.0;
        self.text(MARGIN, s, Font::Bold, 11.0, 0.2);
        self.y -= 4.0;
        let _ = writeln!(
            self.cur,
            "0.57 0.61 0.64 RG 0.6 w {MARGIN} {} m {} {} l S",
            self.y,
            W - MARGIN,
            self.y
        );
        self.y -= 12.0;
    }
    fn kv(&mut self, k: &str, v: &str) {
        let lines = wrap(v, Font::Regular, 8.5, W - 2.0 * MARGIN - 150.0);
        self.need(11.0 * lines.len() as f32);
        self.text(MARGIN, k, Font::Bold, 8.5, 0.15);
        for l in lines {
            self.text(MARGIN + 150.0, &l, Font::Regular, 8.5, 0.0);
            self.y -= 11.0;
        }
    }
    /// Table row with fixed column widths; cells wrap within columns.
    fn row(&mut self, cells: &[&str], widths: &[f32], font: Font, shade: bool) {
        let size = 7.5;
        let wrapped: Vec<Vec<String>> = cells
            .iter()
            .zip(widths)
            .map(|(c, w)| wrap(c, font, size, w - 6.0))
            .collect();
        let n = wrapped.iter().map(|w| w.len()).max().unwrap_or(1);
        let h = n as f32 * (size + 2.0) + 4.0;
        self.need(h);
        if shade {
            let _ = writeln!(
                self.cur,
                "0.94 0.95 0.96 rg {MARGIN} {} {} {h} re f 0 g",
                self.y - h + size + 2.0,
                W - 2.0 * MARGIN
            );
        }
        let mut x = MARGIN;
        for (col, w) in wrapped.iter().zip(widths) {
            let mut yy = self.y;
            for l in col {
                let _ = writeln!(
                    self.cur,
                    "BT /{} {size} Tf {} {yy} Td ({}) Tj ET",
                    font.res(),
                    x + 3.0,
                    pdf_text(l)
                );
                yy -= size + 2.0;
            }
            x += w;
        }
        self.y -= h;
    }
    fn into_bytes(mut self) -> Vec<u8> {
        if !self.cur.is_empty() || self.pages.is_empty() {
            self.finish_page();
        }
        let mut objs: Vec<Vec<u8>> = Vec::new();
        // 1 catalog, 2 pages, 3-5 fonts, then (page, content) pairs.
        let n = self.pages.len();
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 6 + 2 * i)).collect();
        objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        objs.push(format!("<< /Type /Pages /Count {n} /Kids [{}] >>", kids.join(" ")).into_bytes());
        for f in ["Helvetica", "Helvetica-Bold", "Courier"] {
            objs.push(
                format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /{f} /Encoding /WinAnsiEncoding >>"
                )
                .into_bytes(),
            );
        }
        for (i, content) in self.pages.iter().enumerate() {
            objs.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W} {H}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> /Contents {} 0 R >>",
                    7 + 2 * i
                )
                .into_bytes(),
            );
            let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
            stream.extend_from_slice(content.as_bytes());
            stream.extend_from_slice(b"\nendstream");
            objs.push(stream);
        }
        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes(),
        );
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}

/// Render the report as PDF bytes.
pub fn to_pdf(d: &ReportData, audience: Audience) -> Vec<u8> {
    let internal = audience == Audience::Internal;
    let title = if internal {
        "Factory / Engineering Validation Report"
    } else {
        "System Diagnostic Report"
    };
    let mut doc = Doc::new(format!(
        "{} {} | run {} | generated {} | not UL/FCC/CE certification",
        be_core::PRODUCT_SHORT_NAME,
        d.engine_version,
        d.run.id,
        d.generated_utc.to_rfc3339()
    ));
    doc.text(MARGIN, &d.product, Font::Bold, 15.0, 0.2);
    doc.y -= 18.0;
    doc.text(
        MARGIN,
        &format!("{title} - {}", be_core::PRODUCT_SUBTITLE),
        Font::Regular,
        9.5,
        0.3,
    );
    doc.y -= 18.0;
    doc.para(
        &format!("Report status: {}", d.completeness),
        Font::Bold,
        9.0,
    );
    if let Some(e) = &d.evaluation {
        let current = d
            .unit
            .as_ref()
            .map(|u| u.release_state.label())
            .unwrap_or("-");
        doc.para(
            &format!(
                "Shipment disposition (current): {current}. Policy evaluation of this run: {}{}",
                e.recommended_state.label(),
                if e.eligible_for_review {
                    " (eligible for authorized release review)"
                } else {
                    ""
                }
            ),
            Font::Bold,
            9.0,
        );
    }
    doc.heading("System identity");
    if let Some(u) = &d.unit {
        doc.kv("Unit serial", &u.serial);
        doc.kv("Configuration", u.configuration.as_deref().unwrap_or("-"));
    } else {
        doc.kv("Unit", "Not associated with a manufacturing record");
    }
    if let Some(inv) = &d.inventory {
        doc.kv(
            "System",
            &format!(
                "{} {}",
                inv.system.manufacturer.clone().unwrap_or("unknown".into()),
                inv.system.product.clone().unwrap_or_default()
            ),
        );
        doc.kv(
            "CPU",
            inv.cpu
                .as_ref()
                .and_then(|c| c.model.clone())
                .as_deref()
                .unwrap_or("unknown"),
        );
        doc.kv(
            "Memory (OS visible)",
            &inv.memory
                .as_ref()
                .map(|m| format_bytes(m.total_bytes))
                .unwrap_or("unknown".into()),
        );
        doc.kv(
            "GPUs",
            &inv.gpus
                .iter()
                .map(|g| g.name.clone().unwrap_or(g.id.clone()))
                .collect::<Vec<_>>()
                .join("; "),
        );
        doc.kv(
            "Operating system",
            &format!(
                "{} ({})",
                inv.os.name.clone().unwrap_or("unknown".into()),
                inv.os.kernel.clone().unwrap_or_default()
            ),
        );
        doc.kv(
            "BIOS",
            inv.firmware.bios_version.as_deref().unwrap_or("unknown"),
        );
    }
    doc.heading("Run");
    doc.kv(
        "Run ID / profile",
        &format!(
            "{} / {} version {}",
            d.run.id, d.run.profile_id, d.run.profile_version
        ),
    );
    doc.kv("Engine version", &d.run.engine_version);
    doc.kv(
        "Started / ended (UTC)",
        &format!(
            "{} / {}",
            d.run.started_utc.to_rfc3339(),
            d.run
                .ended_utc
                .map(|t| t.to_rfc3339())
                .unwrap_or("not recorded".into())
        ),
    );
    doc.kv("Run state", &d.run.state);
    if internal {
        doc.kv(
            "Station / operator",
            &format!("{} / {}", d.run.station, d.run.operator),
        );
        doc.kv(
            "High-load authorized",
            if d.run.high_load_authorized {
                "yes"
            } else {
                "no"
            },
        );
    }
    doc.heading("Results");
    let widths = [150.0, 70.0, 55.0, 75.0, 161.0];
    doc.row(
        &["Step / test", "Result", "Duration", "Code", "Coverage"],
        &widths,
        Font::Bold,
        true,
    );
    for (i, (k, r)) in d.results.iter().enumerate() {
        let codes_s = r
            .faults
            .iter()
            .map(|f| f.code.clone())
            .collect::<Vec<_>>()
            .join(" ");
        let dur = format_duration_ms(r.duration_ms);
        let label = format!("{k} ({})", r.test_id);
        doc.row(
            &[&label, r.status.label(), &dur, &codes_s, &r.coverage],
            &widths,
            Font::Regular,
            i % 2 == 1,
        );
    }
    let faults: Vec<_> = d
        .results
        .iter()
        .flat_map(|(k, r)| r.faults.iter().map(move |f| (k, f)))
        .collect();
    doc.heading("Faults and observations");
    if faults.is_empty() {
        doc.para("No faults recorded.", Font::Regular, 8.5);
    }
    for (k, f) in faults.iter().take(200) {
        let cat = codes::lookup(&f.code);
        doc.para(
            &format!("{} [{}] {}: {}", f.code, k, f.message, f.observation),
            Font::Bold,
            8.0,
        );
        if let Some(c) = cat {
            doc.para(
                &format!(
                    "Possible causes (not a diagnosis): {}. Next safe steps: {}.",
                    c.possible_causes.join("; "),
                    c.next_steps.join("; ")
                ),
                Font::Regular,
                7.5,
            );
        }
    }
    if !d.fault_isolation.findings.is_empty() {
        doc.heading(&format!(
            "Fault isolation (ruleset {})",
            d.fault_isolation.ruleset_version
        ));
        doc.para(&d.fault_isolation.caveat, Font::Regular, 7.5);
        for f in &d.fault_isolation.findings {
            doc.para(
                &format!("{}{}", f.title, if f.hold { " - hold unit" } else { "" }),
                Font::Bold,
                8.5,
            );
            for h in &f.hypotheses {
                doc.para(
                    &format!("  [{}] {}", h.confidence.label(), h.cause),
                    Font::Regular,
                    8.0,
                );
            }
            doc.para(
                &format!(
                    "  Next safe tests: {}. Escalation: {}",
                    f.next_tests.join(", "),
                    f.escalation
                ),
                Font::Regular,
                8.0,
            );
        }
    }
    doc.heading("Measurements");
    let mw = [130.0, 220.0, 110.0, 51.0];
    doc.row(
        &["Step", "Measurement", "Value", "Quality"],
        &mw,
        Font::Bold,
        true,
    );
    let mut i = 0;
    for (k, r) in &d.results {
        for m in &r.measurements {
            let q = match m.quality {
                DataQuality::Measured => "measured",
                DataQuality::Derived => "derived",
                DataQuality::Unavailable => "UNAVAILABLE",
                DataQuality::Stale => "stale",
            };
            let v = m.display_value();
            doc.row(&[k, &m.name, &v, q], &mw, Font::Regular, i % 2 == 1);
            i += 1;
        }
    }
    if let Some(e) = &d.evaluation {
        doc.heading("Release policy evaluation");
        if e.blockers.is_empty() {
            doc.para(
                "No blockers. Release still requires a deliberate authorized approval.",
                Font::Regular,
                8.5,
            );
        }
        for b in &e.blockers {
            doc.para(&format!("- {b}"), Font::Regular, 8.5);
        }
    }
    if internal && !d.dispositions.is_empty() {
        doc.heading("Disposition history");
        for x in &d.dispositions {
            doc.para(
                &format!(
                    "{} {} by {}: {}",
                    x.ts.to_rfc3339(),
                    x.state.label(),
                    x.actor,
                    x.reason
                ),
                Font::Regular,
                8.0,
            );
        }
    }
    if internal && !d.events.is_empty() {
        doc.heading("Event log (most recent 80 entries)");
        let start = d.events.len().saturating_sub(80);
        for e in &d.events[start..] {
            doc.para(
                &format!("{} {}", e.ts.to_rfc3339(), e.line()),
                Font::Mono,
                6.5,
            );
        }
    }
    doc.heading("Notice");
    doc.para(
        "Factory acceptance by this software is not UL, FCC, CE or other independent certification and does not replace electrical safety testing or calibrated instruments. A generated report is not by itself evidence that tests passed; see result statuses above.",
        Font::Regular,
        8.0,
    );
    let any_fail = d.results.iter().any(|(_, r)| r.status == TestStatus::Fail);
    if any_fail {
        doc.para("This run contains FAILED tests.", Font::Bold, 8.0);
    }
    doc.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_and_wrapping() {
        assert_eq!(pdf_text("a(b)c\\ 45°C — ok"), "a\\(b\\)c\\\\ 45\\260C - ok");
        assert_eq!(pdf_text("日本"), "??");
        let w = wrap("aaaa bbbb cccc dddd", Font::Regular, 10.0, 60.0);
        assert!(w.len() > 1);
        assert!(wrap(&"x".repeat(500), Font::Mono, 10.0, 100.0).len() > 5);
    }

    #[test]
    fn xref_offsets_point_at_objects() {
        let mut d = Doc::new("f".into());
        for i in 0..200 {
            d.para(&format!("line {i}"), Font::Regular, 9.0);
        }
        let b = d.into_bytes();
        // Work on raw bytes: the binary header comment is not UTF-8.
        let find = |hay: &[u8], pat: &[u8]| hay.windows(pat.len()).rposition(|w| w == pat);
        assert!(b.starts_with(b"%PDF-1.4"));
        assert!(b.ends_with(b"%%EOF\n"));
        let p = find(&b, b"startxref\n").unwrap() + 10;
        let sx: usize = std::str::from_utf8(&b[p..])
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(b[sx..].starts_with(b"xref"));
        let table = std::str::from_utf8(&b[sx..]).unwrap();
        for (i, line) in table
            .lines()
            .skip(3)
            .take_while(|l| l.ends_with(" n "))
            .enumerate()
        {
            let off: usize = line[..10].parse().unwrap();
            assert!(
                b[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()),
                "object {}",
                i + 1
            );
        }
        let s = String::from_utf8_lossy(&b);
        let count: usize = s
            .split("/Count ")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(count > 1, "200 lines must span several pages, got {count}");
    }
}
