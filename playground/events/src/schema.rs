//! Event validation: schema 1, as specified in `playground/events/SCHEMA.md`.
//!
//! Every field the collector keeps is listed here with its type and limits.
//! Unknown fields (in the envelope or in `props`) are dropped and counted; a
//! known field with the wrong type or out of range rejects the event. Nothing
//! is ever copied from the request except the fields below.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub const SCHEMA_VERSION: i64 = 1;
/// Whole request body. Larger bodies get 413 before parsing. Always
/// leaves room for one largest-possible `program` event plus ordinary
/// events (checked at compile time below, and by a test).
pub const MAX_BODY_BYTES: usize = 256 * 1024;
pub const MAX_EVENTS_PER_BATCH: usize = 100;
/// One serialized event (as received, after JSON escaping).
pub const MAX_EVENT_BYTES: usize = 24 * 1024;
/// Program source text, in UTF-8 bytes (before JSON escaping).
pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
/// One serialized `program` event: the envelope plus the source, which
/// JSON escaping can double (quotes, backslashes, newlines).
pub const MAX_PROGRAM_EVENT_BYTES: usize = 2 * MAX_SOURCE_BYTES + MAX_EVENT_BYTES;
// A maximal program event always fits in one request, with room for a few
// ordinary events beside it (checked at compile time).
const _: () = assert!(MAX_PROGRAM_EVENT_BYTES + 4 * MAX_EVENT_BYTES <= MAX_BODY_BYTES);

const MAX_DURATION_MS: i64 = 86_400_000;

#[derive(Clone, Copy)]
enum Charset {
    /// `[0-9A-Za-z.+-]`
    Version,
    /// `[A-Za-z0-9-]`
    Lang,
    /// `[a-z0-9.-]`
    Host,
    /// `[a-z0-9-]`
    Kebab,
    /// `[A-Za-z0-9_.:-]` (a source location like `expr_visitor.rs:862`)
    Location,
    /// Anything but control characters.
    Text,
}

impl Charset {
    fn allows(self, c: char) -> bool {
        match self {
            Charset::Version => c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'),
            Charset::Lang => c.is_ascii_alphanumeric() || c == '-',
            Charset::Host => c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-'),
            Charset::Kebab => c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-',
            Charset::Location => c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'),
            Charset::Text => !c.is_control(),
        }
    }
}

#[derive(Clone, Copy)]
enum Ty {
    Uuid,
    /// Client ms epoch.
    Ts,
    /// Duration in ms, `0..=86_400_000`.
    Dur,
    /// 1-32 of `[a-z0-9_]`.
    Token,
    /// 16-64 lowercase hex.
    FigureId,
    /// 64 lowercase hex (SHA-256).
    ProgramId,
    /// 1-128 chars, no whitespace or control characters.
    ElementId,
    Int(i64, i64),
    Bool,
    Enum(&'static [&'static str]),
    /// Non-empty string of at most `n` bytes.
    Str(usize, Charset),
    Surface,
    Page,
    Viewport,
    /// Array of at most `n` `[dt_ms, x, y]` triples.
    Samples(usize),
    ErrorCodes,
    Answers,
    /// Program source text, at most MAX_SOURCE_BYTES (may be empty).
    Source,
}

struct Field {
    name: &'static str,
    ty: Ty,
    required: bool,
}

const fn req(name: &'static str, ty: Ty) -> Field {
    Field {
        name,
        ty,
        required: true,
    }
}
const fn opt(name: &'static str, ty: Ty) -> Field {
    Field {
        name,
        ty,
        required: false,
    }
}

/// Envelope fields other than `event` and `props`, which are handled apart.
const ENVELOPE: &[Field] = &[
    req("schema", Ty::Int(SCHEMA_VERSION, SCHEMA_VERSION)),
    req("event_id", Ty::Uuid),
    req("seq", Ty::Int(0, i32::MAX as i64)),
    req("client_ts", Ty::Ts),
    req("surface", Ty::Surface),
    req("page", Ty::Page),
    req("page_load_id", Ty::Uuid),
    req("session_id", Ty::Uuid),
    req("anon_id", Ty::Uuid),
    req("consent_mode", Ty::Enum(&["notice", "opt_in"])),
    req("tier", Ty::Int(1, 2)),
    req("rv_version", Ty::Str(32, Charset::Version)),
    req("client_version", Ty::Str(32, Charset::Version)),
    opt("viewport", Ty::Viewport),
    opt("lang", Ty::Str(35, Charset::Lang)),
    opt("tz_offset", Ty::Int(-840, 840)),
    opt("referrer_host", Ty::Str(253, Charset::Host)),
    opt("cohort", Ty::Str(40, Charset::Kebab)),
];

/// The fixed event vocabulary. Order matches SCHEMA.md section 3.
pub const EVENT_NAMES: &[&str] = &[
    "page_view",
    "page_hide",
    "figure_view",
    "tooltip_dwell",
    "code_line_hover",
    "pointer_trace",
    "ui",
    "program",
    "edit",
    "run",
    "run_result",
    "consent",
    "withdraw",
    "survey",
];

/// A `&'static [Field]` built in a const context (`const fn` calls are not
/// promoted to statics on their own).
macro_rules! spec {
    ($($f:expr),* $(,)?) => {{
        const F: &[Field] = &[$($f),*];
        F
    }};
}

fn props_spec(event: &str) -> Option<&'static [Field]> {
    const FIG_OPT_PROGRAM: Field = opt("program_id", Ty::ProgramId);
    Some(match event {
        "page_view" => spec![opt(
            "nav_type",
            Ty::Enum(&["navigate", "reload", "back_forward", "prerender"])
        )],
        "page_hide" => spec![
            req("active_ms", Ty::Dur),
            req("total_ms", Ty::Dur),
            req("max_scroll_pct", Ty::Int(0, 100)),
            req("reason", Ty::Enum(&["hidden", "unload"])),
        ],
        "figure_view" => spec![
            req("figure_id", Ty::FigureId),
            req("figure_index", Ty::Int(0, 999)),
            req("enter_ts", Ty::Ts),
            req("leave_ts", Ty::Ts),
            req("visible_ms", Ty::Dur),
            FIG_OPT_PROGRAM,
        ],
        "tooltip_dwell" => spec![
            req("figure_id", Ty::FigureId),
            req("element_id", Ty::ElementId),
            req(
                "element_kind",
                Ty::Enum(&[
                    "header",
                    "dot",
                    "call",
                    "fn",
                    "arrow",
                    "line",
                    "branch_line",
                    "stop_bar",
                    "bracket",
                    "code_line",
                    "other",
                ]),
            ),
            opt("event_kind", Ty::Token),
            opt("var", Ty::Str(64, Charset::Text)),
            opt("line", Ty::Int(1, 100_000)),
            opt("line_end", Ty::Int(1, 100_000)),
            opt("state", Ty::Token),
            opt("drop", Ty::Bool),
            req("enter_ts", Ty::Ts),
            req("leave_ts", Ty::Ts),
            req("dwell_ms", Ty::Dur),
            FIG_OPT_PROGRAM,
        ],
        "code_line_hover" => spec![
            req("figure_id", Ty::FigureId),
            req("line", Ty::Int(1, 100_000)),
            req("enter_ts", Ty::Ts),
            req("leave_ts", Ty::Ts),
            req("dwell_ms", Ty::Dur),
            FIG_OPT_PROGRAM,
        ],
        "pointer_trace" => spec![
            req("figure_id", Ty::FigureId),
            req("chunk", Ty::Int(0, 100_000)),
            req("t0", Ty::Ts),
            req("input", Ty::Enum(&["mouse", "touch", "pen"])),
            req("samples", Ty::Samples(600)),
            opt("taps", Ty::Samples(100)),
            FIG_OPT_PROGRAM,
        ],
        "ui" => spec![
            req(
                "action",
                Ty::Enum(&[
                    "theme_change",
                    "panel_resize",
                    "open_in_playground",
                    "example_select",
                    "import",
                    "export",
                    "share",
                    "privacy_panel",
                    "other",
                ]),
            ),
            opt("value", Ty::Str(64, Charset::Text)),
            opt("source_figure_id", Ty::FigureId),
        ],
        "program" => spec![
            req("program_id", Ty::ProgramId),
            req("source", Ty::Source),
            opt("source_figure_id", Ty::FigureId),
        ],
        "edit" => spec![
            opt("ms_since_last_run", Ty::Dur),
            req("chars_changed", Ty::Int(0, 1_000_000)),
            opt("program_id", Ty::ProgramId),
            opt("program_omitted", Ty::Enum(&["too_large"])),
        ],
        "run" => spec![
            req("run_id", Ty::Uuid),
            req("program_id", Ty::ProgramId),
            opt("figure_id", Ty::FigureId),
            req("line_count", Ty::Int(0, 100_000)),
            opt("example", Ty::Str(64, Charset::Text)),
            opt("source_figure_id", Ty::FigureId),
            opt("program_omitted", Ty::Enum(&["too_large"])),
        ],
        "run_result" => spec![
            req("run_id", Ty::Uuid),
            opt("program_id", Ty::ProgramId),
            opt("figure_id", Ty::FigureId),
            req(
                "outcome",
                Ty::Enum(&[
                    "ok",
                    "compile_error",
                    "refused",
                    "crash",
                    "timeout",
                    "rate_limited",
                    "network_error",
                    "server_error",
                ]),
            ),
            opt("error_codes", Ty::ErrorCodes),
            opt("error_kind", Ty::Str(64, Charset::Kebab)),
            opt("crash_at", Ty::Str(64, Charset::Location)),
            req("latency_ms", Ty::Dur),
            opt("cached", Ty::Bool),
            opt("http_status", Ty::Int(100, 599)),
        ],
        "consent" => spec![
            req("consent_version", Ty::Str(32, Charset::Version)),
            req(
                "choice",
                Ty::Enum(&["opt_in", "notice_shown", "notice_dismissed"])
            ),
            opt("age_18_plus", Ty::Bool),
        ],
        "withdraw" => spec![
            req("consent_version", Ty::Str(32, Charset::Version)),
            req("delete_requested", Ty::Bool),
        ],
        "survey" => spec![
            req("survey_version", Ty::Str(32, Charset::Version)),
            req("answers", Ty::Answers),
        ],
        _ => return None,
    })
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_digit() || ('a'..='f').contains(&c),
        })
}

fn is_lower_hex(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

fn int_in(v: &Value, lo: i64, hi: i64) -> bool {
    v.as_i64().is_some_and(|n| (lo..=hi).contains(&n))
}

fn str_ok(v: &Value, max: usize, cs: Charset) -> bool {
    v.as_str()
        .is_some_and(|s| !s.is_empty() && s.len() <= max && s.chars().all(|c| cs.allows(c)))
}

fn samples_ok(v: &Value, max: usize) -> bool {
    let Some(arr) = v.as_array() else {
        return false;
    };
    arr.len() <= max
        && arr.iter().all(|s| match s.as_array().map(Vec::as_slice) {
            Some([dt, x, y]) => {
                int_in(dt, 0, 600_000)
                    && [x, y].iter().all(|c| {
                        c.as_f64()
                            .is_some_and(|f| f.is_finite() && f.abs() <= 100_000.0)
                    })
            }
            _ => false,
        })
}

fn check(ty: Ty, v: &Value) -> bool {
    match ty {
        Ty::Uuid => v.as_str().is_some_and(is_uuid),
        // > 2020-09 and < 2286: catches seconds-vs-ms mixups.
        Ty::Ts => int_in(v, 1_600_000_000_000, 9_999_999_999_999),
        Ty::Dur => int_in(v, 0, MAX_DURATION_MS),
        Ty::Token => v.as_str().is_some_and(|s| {
            (1..=32).contains(&s.len())
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }),
        Ty::FigureId => v
            .as_str()
            .is_some_and(|s| (16..=64).contains(&s.len()) && is_lower_hex(s)),
        Ty::ProgramId => v.as_str().is_some_and(|s| s.len() == 64 && is_lower_hex(s)),
        Ty::ElementId => v.as_str().is_some_and(|s| {
            (1..=128).contains(&s.chars().count())
                && s.chars().all(|c| !c.is_whitespace() && !c.is_control())
        }),
        Ty::Int(lo, hi) => int_in(v, lo, hi),
        Ty::Bool => v.is_boolean(),
        Ty::Enum(names) => v.as_str().is_some_and(|s| names.contains(&s)),
        Ty::Str(max, cs) => str_ok(v, max, cs),
        Ty::Surface => v.as_str().is_some_and(|s| match s {
            "tutorial" | "playground" => true,
            _ => s.strip_prefix("book:").is_some_and(|host| {
                (1..=64).contains(&host.len()) && host.chars().all(|c| Charset::Host.allows(c))
            }),
        }),
        Ty::Page => v.as_str().is_some_and(|s| {
            s.starts_with('/')
                && s.len() <= 256
                && !s.contains(['?', '#'])
                && s.chars().all(|c| !c.is_control() && !c.is_whitespace())
        }),
        Ty::Viewport => v.as_str().is_some_and(|s| {
            s.split_once('x').is_some_and(|(w, h)| {
                [w, h]
                    .iter()
                    .all(|p| (1..=5).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()))
            })
        }),
        Ty::Samples(max) => samples_ok(v, max),
        Ty::ErrorCodes => v.as_array().is_some_and(|a| {
            a.len() <= 32
                && a.iter().all(|c| {
                    c.as_str().is_some_and(|s| {
                        s.len() == 5
                            && s.starts_with('E')
                            && s[1..].chars().all(|d| d.is_ascii_digit())
                    })
                })
        }),
        Ty::Answers => v.as_object().is_some_and(|m| {
            m.len() <= 20
                && m.iter().all(|(k, val)| {
                    (1..=32).contains(&k.len())
                        && k.chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                        && val
                            .as_str()
                            .is_some_and(|s| s.len() <= 200 && !s.contains(char::is_control))
                })
        }),
        Ty::Source => v.as_str().is_some_and(|s| s.len() <= MAX_SOURCE_BYTES),
    }
}

/// Copy the fields of `spec` from `input` into `out`, checking each.
/// Returns the number of unknown fields skipped, or the name of the first
/// missing/invalid field.
fn copy_fields(
    spec: &[Field],
    input: &Map<String, Value>,
    skip: &[&str],
    out: &mut Map<String, Value>,
) -> Result<u32, String> {
    for f in spec {
        match input.get(f.name) {
            // `null` counts as absent for optional fields.
            Some(Value::Null) | None if !f.required => {}
            None | Some(Value::Null) => return Err(format!("missing:{}", f.name)),
            Some(v) if check(f.ty, v) => {
                out.insert(f.name.to_string(), v.clone());
            }
            Some(_) => return Err(format!("invalid:{}", f.name)),
        }
    }
    let unknown = input
        .keys()
        .filter(|k| !skip.contains(&k.as_str()) && !spec.iter().any(|f| f.name == k.as_str()))
        .count();
    Ok(unknown as u32)
}

/// Server-side policy switches.
#[derive(Clone, Debug, Default)]
pub struct Policy {
    /// Accept `program` events (code text). Off unless RV_EVENTS_ACCEPT_PROGRAMS=1.
    pub accept_programs: bool,
}

/// A validated event, ready for the sink.
#[derive(Debug, Clone, PartialEq)]
pub enum Accepted {
    /// The canonical event record (envelope + `props` + server fields).
    Event(Map<String, Value>),
    /// A program record: code text, stored apart from the event stream and
    /// without any visitor id.
    Program(Map<String, Value>),
}

/// Validate one event and build its record. `server_ts` and `collector` are
/// added here so the record is final. On rejection, returns the reason sent
/// back to the client (`unknown_event`, `missing:<f>`, `invalid:<f>`, ...).
pub fn validate(
    raw: &Value,
    policy: &Policy,
    server_ts: &str,
    collector: &str,
) -> Result<Accepted, String> {
    let Some(input) = raw.as_object() else {
        return Err("not_an_object".into());
    };
    let event = match input.get("event") {
        Some(Value::String(e)) => e.as_str(),
        Some(_) => return Err("invalid:event".into()),
        None => return Err("missing:event".into()),
    };
    let Some(spec) = props_spec(event) else {
        return Err("unknown_event".into());
    };
    let cap = if event == "program" {
        MAX_PROGRAM_EVENT_BYTES
    } else {
        MAX_EVENT_BYTES
    };
    if serde_json::to_vec(raw).map_or(true, |b| b.len() > cap) {
        return Err("too_large".into());
    }

    let mut out = Map::new();
    let mut dropped = copy_fields(ENVELOPE, input, &["event", "props"], &mut out)?;
    out.insert("event".into(), Value::String(event.to_string()));

    let props_in = match input.get("props") {
        Some(Value::Object(p)) => p,
        Some(_) => return Err("invalid:props".into()),
        None => return Err("missing:props".into()),
    };
    let mut props = Map::new();
    dropped += copy_fields(spec, props_in, &[], &mut props)?;

    let tier = out.get("tier").and_then(Value::as_i64).unwrap_or(1);
    if tier < 2 && out.remove("cohort").is_some() {
        dropped += 1;
    }
    if event == "survey" && tier < 2 {
        return Err("tier".into());
    }

    let stamp = |m: &mut Map<String, Value>| {
        m.insert("server_ts".into(), Value::String(server_ts.to_string()));
        m.insert("collector".into(), Value::String(collector.to_string()));
    };

    if event == "program" {
        if !policy.accept_programs {
            return Err("programs_disabled".into());
        }
        let source = props["source"].as_str().unwrap_or_default();
        let digest = Sha256::digest(source.as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        if props["program_id"].as_str() != Some(hex.as_str()) {
            return Err("invalid:program_id".into());
        }
        // Code text is kept apart from who wrote it: the program record has
        // no anon_id/session_id/page_load_id. Runs reference it by id.
        let mut rec = Map::new();
        rec.insert("program_id".into(), Value::String(hex));
        rec.insert("source".into(), props["source"].clone());
        if let Some(f) = props.get("source_figure_id") {
            rec.insert("source_figure_id".into(), f.clone());
        }
        rec.insert("rv_version".into(), out["rv_version"].clone());
        rec.insert("surface".into(), out["surface"].clone());
        stamp(&mut rec);
        return Ok(Accepted::Program(rec));
    }

    out.insert("props".into(), Value::Object(props));
    stamp(&mut out);
    if dropped > 0 {
        out.insert("dropped_fields".into(), Value::from(dropped));
    }
    Ok(Accepted::Event(out))
}

/// Parse a batch body: `{ "events": [...] }`, nothing else at the top level.
pub fn parse_batch(body: &[u8]) -> Result<Vec<Value>, BatchError> {
    let v: Value = serde_json::from_slice(body).map_err(|_| BatchError::Malformed)?;
    let Value::Object(mut top) = v else {
        return Err(BatchError::Malformed);
    };
    if top.len() != 1 {
        return Err(BatchError::Malformed);
    }
    match top.remove("events") {
        Some(Value::Array(events)) if events.len() > MAX_EVENTS_PER_BATCH => {
            Err(BatchError::TooMany)
        }
        Some(Value::Array(events)) => Ok(events),
        _ => Err(BatchError::Malformed),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BatchError {
    Malformed,
    TooMany,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_shape() {
        assert!(is_uuid("0b6b3c1e-6f0e-4a59-9a43-5d1f4c2a9e01"));
        assert!(!is_uuid("0B6B3C1E-6F0E-4A59-9A43-5D1F4C2A9E01"));
        assert!(!is_uuid("0b6b3c1e6f0e4a599a435d1f4c2a9e01"));
    }

    #[test]
    fn every_event_name_has_a_spec() {
        for e in EVENT_NAMES {
            assert!(props_spec(e).is_some(), "{e}");
        }
        assert!(props_spec("pageview").is_none());
    }
}
