//! RustViz 3 spike (not for merge): run the rustc oracle on today's
//! drawings and on the MIR fact layer's states for the programs in
//! `$RV3_DIR` (made by `spike/rv3/run.py`), and compare the two.
//! `RV3_DIR=<dir> cargo test -p rustviz-lib --test rv3_spike -- --nocapture`
mod common;
use common::oracle::{check, check_states, drawn_states};
use std::collections::BTreeMap;
use std::fmt::Write;

/// The fact layer's state for each (row, owner place): own / shared / mut
/// / moved / maybe / partly / unassigned. References are left out, as the
/// oracle leaves out reference columns of a drawing.
fn proto_states(facts: &serde_json::Value) -> Vec<(u32, String, String)> {
    let mut out = Vec::new();
    for body in facts.as_array().into_iter().flatten() {
        let owners: Vec<String> = body["vars"].as_array().into_iter().flatten()
            .filter(|v| v["kind"] == "owner").map(|v| v["name"].as_str().unwrap().to_string()).collect();
        for row in body["rows"].as_array().into_iter().flatten() {
            let line = row["line"].as_u64().unwrap() as u32;
            let lent = row["lent"].as_object().cloned().unwrap_or_default();
            for (place, st) in row["state"].as_object().into_iter().flatten() {
                let root = place.split('.').next().unwrap();
                if !owners.iter().any(|o| o == root) {
                    continue;
                }
                let mut s = st.as_str().unwrap().to_string();
                if s == "own" || s == "partly" {
                    let mut k = "";
                    for (lender, loans) in &lent {
                        let overlaps = lender == place
                            || lender.starts_with(&format!("{place}."))
                            || place.starts_with(&format!("{lender}."));
                        if !overlaps {
                            continue;
                        }
                        for l in loans.as_array().into_iter().flatten() {
                            if l.as_str().unwrap().starts_with("mut") { k = "mut" } else if k.is_empty() { k = "shared" }
                        }
                    }
                    if !k.is_empty() && s == "own" {
                        s = k.to_string();
                    }
                }
                out.push((line, place.clone(), s));
            }
        }
    }
    out
}

fn norm_today(d: &str, tip: &str) -> String {
    match d {
        "Usable" => "own".into(),
        "Shared" => "shared".into(),
        "MutLent" => "mut".into(),
        "Moved" if tip.contains("may") => "maybe".into(),
        "Moved" => "moved".into(),
        "PartlyMoved" => "partly".into(),
        "Unassigned" => "unassigned".into(),
        x => x.into(),
    }
}

#[test]
fn rv3_spike_oracle() {
    let Ok(dir) = std::env::var("RV3_DIR") else {
        eprintln!("rv3_spike_oracle: skipped (set RV3_DIR)");
        return;
    };
    let mut keys: Vec<String> = std::fs::read_dir(&dir).unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".facts.json").map(str::to_owned))
        .collect();
    keys.sort();
    let mut report = String::new();
    let mut tsv = String::from("program\ttoday_tested\ttoday_violations\tproto_tested\tproto_violations\tcommon_points\tagree\tdisagree\trv3render_tested\trv3render_violations\n");
    for key in keys {
        let p = |ext: &str| format!("{dir}/{key}{ext}");
        let (Ok(src), Ok(tl), Ok(code), Ok(facts)) = (std::fs::read_to_string(p(".rs")), std::fs::read_to_string(p(".today.svg")),
            std::fs::read_to_string(p(".code.svg")), std::fs::read_to_string(p(".facts.json"))) else { continue };
        let facts: serde_json::Value = serde_json::from_str(&facts).unwrap();
        let (tv, _, tst) = check(&src, &tl, &code);
        let states = proto_states(&facts);
        let (pv, _, pst) = check_states(&src, &states);
        // Agreement on the points both have a state for.
        let today: BTreeMap<(u32, String), String> = drawn_states(&src, &tl, &code).into_iter()
            .map(|(l, v, d, tip)| ((l, v), norm_today(&d, &tip))).collect();
        let proto: BTreeMap<(u32, String), String> = states.iter().map(|(l, v, s)| ((*l, v.clone()), s.clone())).collect();
        let (mut agree, mut disagree, mut common) = (0, 0, 0);
        let _ = writeln!(report, "== {key}: today tested {} / {} violations; proto tested {} / {} violations", tst.tested, tst.violations, pst.tested, pst.violations);
        for (k, t) in &today {
            if let Some(m) = proto.get(k) {
                common += 1;
                if t == m { agree += 1 } else {
                    disagree += 1;
                    let _ = writeln!(report, "   DIFF L{} {}: today={} proto={}", k.0, k.1, t, m);
                }
            }
        }
        for v in &tv {
            let _ = writeln!(report, "   TODAY  {} {}", v.check, v.text);
        }
        for v in &pv {
            let _ = writeln!(report, "   PROTO  {} {}", v.check, v.text);
        }
        // Today's renderer fed by the MIR facts (straight-line bodies only).
        let (mut rt, mut rv) = (String::from("-"), String::from("-"));
        if let (Ok(r), Ok(rc)) = (std::fs::read_to_string(p(".rv3.svg")), std::fs::read_to_string(p(".rv3code.svg"))) {
            if r != tl {
                let (vs, _, st) = check(&src, &r, &rc);
                rt = st.tested.to_string();
                rv = st.violations.to_string();
                for v in &vs {
                    let _ = writeln!(report, "   RV3RENDER  {} {}", v.check, v.text);
                }
            }
        }
        let _ = writeln!(tsv, "{key}\t{}\t{}\t{}\t{}\t{common}\t{agree}\t{disagree}\t{rt}\t{rv}", tst.tested, tst.violations, pst.tested, pst.violations);
    }
    std::fs::write(format!("{dir}/oracle-report.txt"), &report).unwrap();
    std::fs::write(format!("{dir}/oracle-summary.tsv"), &tsv).unwrap();
    eprintln!("{tsv}\n{report}");
}
