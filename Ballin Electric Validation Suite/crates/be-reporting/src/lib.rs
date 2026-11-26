//! Report generation. A generated report is never evidence that tests
//! passed: every report states completeness, provenance and disposition.

pub mod pdf;

use be_core::codes;
use be_core::inventory::{Inventory, format_bytes};
use be_core::model::{DataQuality, EventRecord, Provenance, TestResult};
use be_core::policy::{ReleaseEvaluation, ReleaseState};
use be_core::status::TestStatus;
use be_core::time::{UtcMillis, format_duration_ms};
use be_storage::{Disposition, RunRecord, ServiceRecord, Store, Unit};
use serde::Serialize;
use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// Full factory/engineering detail.
    Internal,
    /// Redacted: no operator/station identities, notes or audit details.
    Customer,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportData {
    pub schema: u32,
    pub generated_utc: UtcMillis,
    pub product: String,
    pub engine_version: String,
    pub run: RunRecord,
    pub unit: Option<Unit>,
    pub results: Vec<(String, TestResult)>,
    pub events: Vec<EventRecord>,
    pub evaluation: Option<ReleaseEvaluation>,
    pub dispositions: Vec<Disposition>,
    pub service: Vec<ServiceRecord>,
    pub rework_chain: Vec<i64>,
    pub inventory: Option<Inventory>,
    pub completeness: String,
    pub fault_isolation: be_core::isolation::Analysis,
}

impl ReportData {
    pub fn from_store(store: &Store, run_id: i64) -> Result<ReportData, String> {
        let run = store
            .get_run(run_id)
            .map_err(|e| e.to_string())?
            .ok_or(format!("run {run_id} not found"))?;
        let results = store.results_for_run(run_id).map_err(|e| e.to_string())?;
        let events = store
            .events(Some(run_id), 5000)
            .map_err(|e| e.to_string())?;
        let unit = match &run.unit_serial {
            Some(s) => store.get_unit(s).map_err(|e| e.to_string())?,
            None => None,
        };
        let (dispositions, service, evaluation) = match &run.unit_serial {
            Some(s) => {
                let profile: Option<be_core::profile::Profile> =
                    serde_json::from_str(&run.profile_json).ok();
                let eval = profile.map(|p| {
                    let map = results.iter().cloned().collect();
                    let manual = store.manual_checks(s, Some(run_id)).unwrap_or_default();
                    let waivers = store.waivers(s, run_id).unwrap_or_default();
                    let mut evaluation = be_core::policy::evaluate(&p, &map, &manual, &waivers);
                    if run.fixture || run.state != "completed" {
                        evaluation.eligible_for_review = false;
                        evaluation.recommended_state = ReleaseState::Hold;
                        evaluation
                            .blockers
                            .push("Release requires a completed live run".into());
                    }
                    evaluation
                });
                (
                    store.dispositions(s).unwrap_or_default(),
                    store.service_records(s).unwrap_or_default(),
                    eval,
                )
            }
            None => (vec![], vec![], None),
        };
        let mut chain = vec![];
        let mut cur = run.parent_run_id;
        while let Some(p) = cur {
            chain.push(p);
            cur = store
                .get_run(p)
                .ok()
                .flatten()
                .and_then(|r| r.parent_run_id);
            if chain.len() > 100 {
                break;
            }
        }
        let inventory = run
            .inventory_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok());
        let completeness = completeness(&run, &results);
        let fault_isolation = be_core::isolation::analyze(&results);
        Ok(ReportData {
            schema: be_core::RESULT_SCHEMA_VERSION,
            generated_utc: UtcMillis::now(),
            product: be_core::PRODUCT_NAME.into(),
            engine_version: be_core::ENGINE_VERSION.into(),
            run,
            unit,
            results,
            events,
            evaluation,
            dispositions,
            service,
            rework_chain: chain,
            inventory,
            completeness,
            fault_isolation,
        })
    }
}

fn completeness(run: &RunRecord, results: &[(String, TestResult)]) -> String {
    let mut flags = Vec::new();
    if run.fixture
        || results
            .iter()
            .any(|(_, r)| r.provenance == Provenance::Fixture)
    {
        flags.push("SIMULATED / FIXTURE DATA — NOT VALID FOR RELEASE");
    }
    let recorded: std::collections::HashMap<_, _> = results
        .iter()
        .map(|(key, r)| (key.as_str(), r.test_id.as_str()))
        .collect();
    let complete =
        serde_json::from_str::<be_core::profile::Profile>(&run.profile_json).is_ok_and(|p| {
            !p.steps.is_empty()
                && p.steps.len() == results.len()
                && p.steps
                    .iter()
                    .all(|s| recorded.get(s.key()).copied() == Some(s.test.as_str()))
        });
    if run.state != "completed" || !complete {
        flags.push("INCOMPLETE RUN");
    }
    if results.iter().any(|(_, r)| r.status != TestStatus::Pass) {
        flags.push("NOT ALL TESTS PASSED");
    }
    if flags.is_empty() {
        "COMPLETE — all recorded tests passed".into()
    } else {
        flags.join("; ")
    }
}

pub fn to_json(d: &ReportData, audience: Audience) -> String {
    let mut v = serde_json::to_value(d).unwrap_or_default();
    if audience == Audience::Customer {
        redact_json(&mut v);
    }
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

fn redact_json(v: &mut serde_json::Value) {
    if let Some(run) = v.get_mut("run").and_then(|r| r.as_object_mut()) {
        for k in ["operator", "station", "profile_json", "inventory_json"] {
            run.remove(k);
        }
    }
    if let Some(o) = v.as_object_mut() {
        if let Some(evaluation) = o.get_mut("evaluation").and_then(|e| e.as_object_mut()) {
            evaluation.remove("notes");
        }
        o.remove("events");
        o.remove("dispositions");
        if let Some(u) = o.get_mut("unit").and_then(|u| u.as_object_mut()) {
            u.remove("assembly_technician");
            u.remove("order_ref");
            u.remove("bom_json");
        }
        if let Some(svc) = o.get_mut("service").and_then(|s| s.as_array_mut()) {
            for s in svc {
                if let Some(so) = s.as_object_mut() {
                    so.remove("technician");
                }
            }
        }
        if let Some(inv) = o.get_mut("inventory").and_then(|i| i.as_object_mut()) {
            if let Some(sys) = inv.get_mut("system").and_then(|s| s.as_object_mut()) {
                sys.remove("hostname");
                sys.remove("serial");
            }
            if let Some(disks) = inv.get_mut("storage").and_then(|v| v.as_array_mut()) {
                for disk in disks {
                    if let Some(fields) = disk.as_object_mut() {
                        fields.remove("serial");
                    }
                }
            }
            if let Some(net) = inv.get_mut("network").and_then(|n| n.as_array_mut()) {
                for n in net {
                    if let Some(no) = n.as_object_mut() {
                        no.remove("mac");
                    }
                }
            }
        }
    }
    if let Some(rs) = v.get_mut("results").and_then(|r| r.as_array_mut()) {
        for pair in rs {
            if let Some(r) = pair.get_mut(1).and_then(|r| r.as_object_mut()) {
                r.remove("notes");
            }
        }
    }
}

fn csv_field(s: &str) -> String {
    // Neutralize spreadsheet formula injection and quote.
    let s = if s.trim_start().starts_with(['=', '+', '-', '@']) || s.starts_with(['\t', '\r', '\n'])
    {
        format!("'{s}")
    } else {
        s.to_string()
    };
    format!("\"{}\"", s.replace('"', "\"\""))
}

pub fn results_csv(d: &ReportData) -> String {
    let mut out = String::from(
        "run_id,step,test_id,test_version,status,device,started_utc,duration_ms,provenance,fault_codes,coverage\n",
    );
    for (k, r) in &d.results {
        let codes: Vec<&str> = r.faults.iter().map(|f| f.code.as_str()).collect();
        let _ = writeln!(
            out,
            "{},{},{},{},{},{},{},{},{},{},{}",
            d.run.id,
            csv_field(k),
            csv_field(&r.test_id),
            csv_field(&r.test_version),
            r.status.label(),
            csv_field(&r.device),
            r.started_utc.to_rfc3339(),
            r.duration_ms,
            match r.provenance {
                Provenance::Live => "live",
                Provenance::Fixture => "fixture",
            },
            csv_field(&codes.join(" ")),
            csv_field(&r.coverage)
        );
    }
    out
}

pub fn measurements_csv(d: &ReportData) -> String {
    let mut out = String::from("run_id,step,measurement,value,text,unit,quality\n");
    for (k, r) in &d.results {
        for m in &r.measurements {
            let _ = writeln!(
                out,
                "{},{},{},{},{},{},{:?}",
                d.run.id,
                csv_field(k),
                csv_field(&m.name),
                m.value.map(|v| v.to_string()).unwrap_or_default(),
                csv_field(m.text.as_deref().unwrap_or("")),
                csv_field(&m.unit),
                m.quality
            );
        }
    }
    out
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn status_class(s: TestStatus) -> &'static str {
    match s {
        TestStatus::Pass => "pass",
        TestStatus::Fail | TestStatus::Aborted => "fail",
        TestStatus::Warning | TestStatus::Inconclusive => "warn",
        TestStatus::Running => "run",
        _ => "pend",
    }
}

pub fn to_html(d: &ReportData, audience: Audience) -> String {
    let internal = audience == Audience::Internal;
    let mut h = String::new();
    let title = if internal {
        "Factory / Engineering Validation Report"
    } else {
        "System Diagnostic Report"
    };
    let _ = write!(
        h,
        r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><title>{} — {}</title>
<style>
body{{font-family:"Segoe UI",Tahoma,"DejaVu Sans",sans-serif;font-size:10pt;color:#202020;margin:16px;}}
h1{{font-size:14pt;margin:0 0 4px 0;color:#34434E}} h2{{font-size:11pt;border-bottom:1px solid #929CA4;margin:16px 0 6px 0;color:#34434E}}
table{{border-collapse:collapse;width:100%;margin-bottom:8px}} th,td{{border:1px solid #929CA4;padding:3px 6px;text-align:left;vertical-align:top}}
th{{background:#E2E5E7}} .pass{{color:#26752A;font-weight:bold}} .fail{{color:#BD1818;font-weight:bold}} .warn{{color:#946600;font-weight:bold}}
.run{{color:#245CA2;font-weight:bold}} .pend{{color:#666;font-weight:bold}} .banner{{border:1px solid #946600;background:#FFF6D8;padding:6px;margin:8px 0}}
.hold{{border:1px solid #BD1818;background:#FBE3E3;padding:6px;margin:8px 0}} .mono{{font-family:Consolas,"DejaVu Sans Mono",monospace;font-size:9pt}}
.small{{color:#555;font-size:8.5pt}} @media print{{body{{margin:0}}}}
</style></head><body>"#,
        esc(be_core::PRODUCT_SHORT_NAME),
        title
    );
    let _ = write!(
        h,
        "<h1>{}</h1><div>{} — {}</div>",
        esc(&d.product),
        esc(title),
        esc(be_core::PRODUCT_SUBTITLE)
    );
    let _ = write!(
        h,
        r#"<div class="banner"><b>Report status:</b> {}</div>"#,
        esc(&d.completeness)
    );
    if let Some(e) = &d.evaluation {
        let cls = if e.recommended_state == ReleaseState::Hold {
            "hold"
        } else {
            "banner"
        };
        let current = d
            .unit
            .as_ref()
            .map(|u| u.release_state.label())
            .unwrap_or("-");
        let _ = write!(
            h,
            r#"<div class="{cls}"><b>Shipment disposition (current):</b> {}<br><b>Policy evaluation of this run:</b> {}{}</div>"#,
            esc(current),
            esc(e.recommended_state.label()),
            if e.eligible_for_review {
                " — eligible for authorized release review"
            } else {
                ""
            }
        );
    }
    h.push_str("<h2>System identity</h2><table>");
    let row = |h: &mut String, k: &str, v: &str| {
        let _ = write!(
            h,
            "<tr><th style=\"width:28%\">{}</th><td>{}</td></tr>",
            esc(k),
            esc(v)
        );
    };
    if let Some(u) = &d.unit {
        row(&mut h, "Unit serial", &u.serial);
        row(
            &mut h,
            "Configuration",
            u.configuration.as_deref().unwrap_or("-"),
        );
        row(
            &mut h,
            "Chassis revision",
            u.chassis_revision.as_deref().unwrap_or("-"),
        );
    } else {
        row(&mut h, "Unit", "Not associated with a manufacturing record");
    }
    if let Some(inv) = &d.inventory {
        row(
            &mut h,
            "Manufacturer / product",
            &format!(
                "{} {}",
                inv.system.manufacturer.clone().unwrap_or("unknown".into()),
                inv.system.product.clone().unwrap_or_default()
            ),
        );
        row(
            &mut h,
            "CPU",
            inv.cpu
                .as_ref()
                .and_then(|c| c.model.clone())
                .as_deref()
                .unwrap_or("unknown"),
        );
        row(
            &mut h,
            "Memory (OS visible)",
            &inv.memory
                .as_ref()
                .map(|m| format_bytes(m.total_bytes))
                .unwrap_or("unknown".into()),
        );
        row(
            &mut h,
            "GPUs",
            &inv.gpus
                .iter()
                .map(|g| g.name.clone().unwrap_or(g.id.clone()))
                .collect::<Vec<_>>()
                .join("; "),
        );
        row(
            &mut h,
            "Storage",
            &inv.storage
                .iter()
                .map(|s| {
                    format!(
                        "{} {}",
                        s.model.clone().unwrap_or(s.name.clone()),
                        s.size_bytes.map(format_bytes).unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("; "),
        );
        row(
            &mut h,
            "Operating system",
            &format!(
                "{} ({})",
                inv.os.name.clone().unwrap_or("unknown".into()),
                inv.os.kernel.clone().unwrap_or_default()
            ),
        );
        row(
            &mut h,
            "BIOS",
            inv.firmware.bios_version.as_deref().unwrap_or("unknown"),
        );
    }
    h.push_str("</table><h2>Run</h2><table>");
    row(&mut h, "Run ID", &d.run.id.to_string());
    row(
        &mut h,
        "Profile",
        &format!("{} version {}", d.run.profile_id, d.run.profile_version),
    );
    row(&mut h, "Engine version", &d.run.engine_version);
    row(&mut h, "Started (UTC)", &d.run.started_utc.to_rfc3339());
    row(
        &mut h,
        "Ended (UTC)",
        &d.run
            .ended_utc
            .map(|t| t.to_rfc3339())
            .unwrap_or("not recorded".into()),
    );
    row(&mut h, "Run state", &d.run.state);
    if internal {
        row(&mut h, "Station", &d.run.station);
        row(&mut h, "Operator", &d.run.operator);
        row(
            &mut h,
            "High-load authorized",
            if d.run.high_load_authorized {
                "yes"
            } else {
                "no"
            },
        );
        row(
            &mut h,
            "Safety profile",
            d.run.safety_profile_id.as_deref().unwrap_or("none"),
        );
        if !d.rework_chain.is_empty() {
            row(
                &mut h,
                "Rework of run(s)",
                &d.rework_chain
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(" ← "),
            );
        }
    }
    h.push_str("</table><h2>Results</h2><table><tr><th>Step</th><th>Test</th><th>Result</th><th>Duration</th><th>Device</th><th>Code</th><th>Coverage</th></tr>");
    for (k, r) in &d.results {
        let _ = write!(
            h,
            r#"<tr><td class="mono">{}</td><td>{} <span class="small">v{}</span></td><td class="{}">{}</td><td>{}</td><td>{}</td><td class="mono">{}</td><td class="small">{}</td></tr>"#,
            esc(k),
            esc(&r.test_id),
            esc(&r.test_version),
            status_class(r.status),
            r.status.label(),
            format_duration_ms(r.duration_ms),
            esc(&r.device),
            esc(&r
                .faults
                .iter()
                .map(|f| f.code.clone())
                .collect::<Vec<_>>()
                .join(" ")),
            esc(&r.coverage)
        );
    }
    h.push_str("</table><h2>Faults and observations</h2>");
    let faults: Vec<_> = d
        .results
        .iter()
        .flat_map(|(k, r)| r.faults.iter().map(move |f| (k, f)))
        .collect();
    if faults.is_empty() {
        h.push_str("<p>No faults recorded.</p>");
    } else {
        h.push_str("<table><tr><th>Step</th><th>Code</th><th>Observation</th><th>Possible causes (not a diagnosis)</th><th>Next safe steps</th></tr>");
        for (k, f) in faults {
            let cat = codes::lookup(&f.code);
            let _ = write!(
                h,
                "<tr><td class=\"mono\">{}</td><td class=\"mono\">{}</td><td>{}: {}</td><td class=\"small\">{}</td><td class=\"small\">{}</td></tr>",
                esc(k),
                esc(&f.code),
                esc(&f.message),
                esc(&f.observation),
                esc(&cat
                    .map(|c| c.possible_causes.join("; "))
                    .unwrap_or_default()),
                esc(&cat.map(|c| c.next_steps.join("; ")).unwrap_or_default())
            );
        }
        h.push_str("</table>");
    }
    let analysis = be_core::isolation::analyze(&d.results);
    if !analysis.findings.is_empty() {
        let _ = write!(
            h,
            "<h2>Fault isolation (ruleset {})</h2><p class=\"small\">{}</p>",
            esc(&analysis.ruleset_version),
            esc(&analysis.caveat)
        );
        for f in &analysis.findings {
            let _ = write!(
                h,
                "<h3 style=\"font-size:10.5pt;margin:8px 0 2px 0\">{}{}</h3><table><tr><th style=\"width:28%\">Confirmed observations</th><td>{}</td></tr>",
                esc(&f.title),
                if f.hold { " — hold unit" } else { "" },
                f.observations
                    .iter()
                    .map(|o| esc(o))
                    .collect::<Vec<_>>()
                    .join("<br>")
            );
            let _ = write!(
                h,
                "<tr><th>Possible causes (not a diagnosis)</th><td>{}</td></tr>",
                f.hypotheses
                    .iter()
                    .map(|x| format!(
                        "[{}] {} <span class=\"small\">({})</span>",
                        x.confidence.label(),
                        esc(&x.cause),
                        esc(&x.evidence.join("; "))
                    ))
                    .collect::<Vec<_>>()
                    .join("<br>")
            );
            let _ = write!(
                h,
                "<tr><th>Next safe tests</th><td class=\"mono\">{}</td></tr><tr><th>Escalation</th><td>{}</td></tr></table>",
                esc(&f.next_tests.join(", ")),
                esc(&f.escalation)
            );
        }
    }
    h.push_str("<h2>Measurements</h2><table><tr><th>Step</th><th>Measurement</th><th>Value</th><th>Quality</th></tr>");
    for (k, r) in &d.results {
        for m in &r.measurements {
            let _ = write!(
                h,
                "<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td><td class=\"small\">{}</td></tr>",
                esc(k),
                esc(&m.name),
                esc(&m.display_value()),
                match m.quality {
                    DataQuality::Measured => "measured",
                    DataQuality::Derived => "derived",
                    DataQuality::Unavailable => "UNAVAILABLE",
                    DataQuality::Stale => "stale",
                }
            );
        }
    }
    h.push_str("</table>");
    if let Some(inv) = &d.inventory
        && !inv.unavailable.is_empty()
    {
        h.push_str(
            "<h2>Unavailable telemetry and data</h2><table><tr><th>Item</th><th>Reason</th></tr>",
        );
        for u in &inv.unavailable {
            let _ = write!(
                h,
                "<tr><td>{}</td><td>{}</td></tr>",
                esc(&u.item),
                esc(&u.reason)
            );
        }
        h.push_str("</table>");
    }
    if let Some(e) = &d.evaluation {
        h.push_str("<h2>Release policy evaluation</h2>");
        if e.blockers.is_empty() {
            h.push_str(
                "<p>No blockers. Release still requires a deliberate authorized approval.</p>",
            );
        } else {
            h.push_str("<ul>");
            for b in &e.blockers {
                let _ = write!(h, "<li>{}</li>", esc(b));
            }
            h.push_str("</ul>");
        }
        if internal && !e.notes.is_empty() {
            h.push_str("<p class=\"small\">");
            for n in &e.notes {
                let _ = write!(h, "{}<br>", esc(n));
            }
            h.push_str("</p>");
        }
    }
    if internal {
        if !d.dispositions.is_empty() {
            h.push_str("<h2>Disposition history</h2><table><tr><th>Time (UTC)</th><th>State</th><th>By</th><th>Reason</th></tr>");
            for x in &d.dispositions {
                let _ = write!(
                    h,
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"small\">{}</td></tr>",
                    x.ts.to_rfc3339(),
                    esc(x.state.label()),
                    esc(&x.actor),
                    esc(&x.reason)
                );
            }
            h.push_str("</table>");
        }
        if !d.service.is_empty() {
            h.push_str("<h2>Service records</h2><table><tr><th>Time (UTC)</th><th>Kind</th><th>Technician</th><th>Description</th><th>Parts</th></tr>");
            for s in &d.service {
                let _ = write!(
                    h,
                    "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    s.ts.to_rfc3339(),
                    esc(&s.kind),
                    esc(&s.technician),
                    esc(&s.description),
                    esc(&s.parts.join(", "))
                );
            }
            h.push_str("</table>");
        }
        h.push_str("<h2>Event log</h2><pre class=\"mono\">");
        for e in &d.events {
            let _ = writeln!(h, "{} {}", e.ts.to_rfc3339(), esc(&e.line()));
        }
        h.push_str("</pre>");
    }
    let _ = write!(
        h,
        r#"<p class="small">Generated {} by {} {}. Report schema {}. Factory acceptance by this software is not UL, FCC, CE or other independent certification and does not replace electrical safety testing or calibrated instruments. A generated report is not by itself evidence that tests passed; see result statuses above.</p></body></html>"#,
        d.generated_utc.to_rfc3339(),
        esc(be_core::PRODUCT_SHORT_NAME),
        esc(&d.engine_version),
        d.schema
    );
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use be_core::model::{FaultRecord, Measurement};
    use be_storage::NewRun;

    fn data() -> ReportData {
        let s = Store::open_in_memory().unwrap();
        let id = s
            .begin_run(&NewRun {
                unit_serial: None,
                parent_run_id: None,
                profile_id: "p".into(),
                profile_version: "1".into(),
                profile_json: "{}".into(),
                station: "secret-station".into(),
                operator: "alice".into(),
                inventory_json: None,
                high_load_authorized: false,
                safety_profile_id: None,
                fixture: true,
            })
            .unwrap();
        let mut r = TestResult::not_executed(
            "BE-T-<x>",
            "1",
            TestStatus::Fail,
            "internal note",
            Provenance::Fixture,
        );
        r.faults.push(FaultRecord::new(
            "BE-MEM-001",
            "mismatch <b>",
            "=cmd|' /C calc'!A0",
        ));
        r.measurements.push(Measurement::unavailable("Temp", "°C"));
        s.insert_result(id, "step<1>", &r).unwrap();
        s.finish_run(id, "completed", "FAIL").unwrap();
        ReportData::from_store(&s, id).unwrap()
    }

    #[test]
    fn html_is_escaped_and_labelled() {
        let d = data();
        let h = to_html(&d, Audience::Internal);
        assert!(h.contains("SIMULATED / FIXTURE DATA"));
        assert!(h.contains("NOT ALL TESTS PASSED"));
        assert!(
            !h.contains("mismatch <b>") && h.contains("mismatch &lt;b&gt;"),
            "user data must be escaped"
        );
        assert!(h.contains("step&lt;1&gt;"));
        assert!(h.contains("UNAVAILABLE"));
        assert!(h.contains("alice"));
        let c = to_html(&d, Audience::Customer);
        assert!(
            !c.contains("alice") && !c.contains("secret-station"),
            "customer report is redacted"
        );
    }

    #[test]
    fn csv_quotes_and_neutralizes_formulas() {
        let d = data();
        let csv = results_csv(&d);
        assert!(csv.lines().count() == 2);
        assert!(csv.contains("\"step<1>\""));
        assert!(csv.contains("FAIL"));
        let m = measurements_csv(&d);
        assert!(m.contains("Unavailable"));
        assert_eq!(csv_field("=1+1"), "\"'=1+1\"");
    }

    #[test]
    fn customer_json_omits_waiver_notes_and_inventory_serials() {
        let mut value = serde_json::json!({
            "evaluation": {"notes": ["reviewer name and reason"], "blockers": ["missing check"]},
            "inventory": {"system": {"serial": "system123"}, "storage": [{"serial": "disk123", "model": "SSD"}]}
        });
        redact_json(&mut value);
        assert_eq!(
            value,
            serde_json::json!({
                "evaluation": {"blockers": ["missing check"]},
                "inventory": {"system": {}, "storage": [{"model": "SSD"}]}
            })
        );
        assert_eq!(csv_field("  =1+1"), "\"'  =1+1\"");
    }

    #[test]
    fn customer_json_redacts_identities() {
        let d = data();
        let j = to_json(&d, Audience::Customer);
        assert!(
            !j.contains("alice") && !j.contains("secret-station") && !j.contains("internal note")
        );
        let i = to_json(&d, Audience::Internal);
        assert!(i.contains("alice"));
    }
}
