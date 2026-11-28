//! Structured `data-rv-*` attributes on every hoverable element of a
//! figure, so a client (telemetry, a replay viewer) can say which
//! element a learner hovered without parsing the tooltip's English,
//! which changes between versions. They change no pixels and no
//! tooltip. The vocabulary is documented in
//! `rustviz-plugin/docs/element-ids.md`; keep the two in sync.
//!
//! The timeline panel's renderers write each element's attributes in an
//! internal form (`Rv::attrs`): the kind, events or state, column names,
//! and the *visual rows* the element sits on. `finalize` then runs once
//! over the finished timeline SVG: it maps rows to the line numbers the
//! code panel shows, gives each element its `data-rv-id`, and makes the
//! ids unique. The code panel writes its final attributes directly
//! (`code_line_attrs`).

use crate::expr_visitor_utils::display_text;
use crate::svg_generator::data::{AltRuntime, BreakAlt, Event, ExternalEvent, LineState, MaybeMovedAt, ResourceAccessPoint, ResourceTy, State, VisualizationData};
use std::collections::HashMap;

/// Element kinds (`data-rv-kind`).
pub const HEADER: &str = "header";
pub const DOT: &str = "dot";
pub const CALL: &str = "call";
pub const FN: &str = "fn";
pub const ARROW: &str = "arrow";
pub const LINE: &str = "line";
pub const BRANCH_LINE: &str = "branch_line";
pub const STOP_BAR: &str = "stop_bar";
pub const BRACKET: &str = "bracket";
pub const CODE_LINE: &str = "code_line";

/// The structured description of one hoverable SVG node.
#[derive(Clone, Default)]
pub struct Rv {
    kind: &'static str,
    events: Vec<&'static str>,
    state: Option<String>,
    var: Option<String>,
    to: Option<String>,
    rows: Option<(usize, usize)>,
    part: Option<&'static str>,
    /// A dot drawn with the drop mark (the column's value is dropped
    /// here).
    drop: bool,
    /// Nodes with the same base id and the same `at` are parts of one
    /// element (a branch segment's lines and its hover polygon), and
    /// share its number when ids are made unique. Without it every node
    /// is an element of its own.
    at: Option<String>,
}

impl Rv {
    pub fn new(kind: &'static str) -> Self {
        Rv { kind, ..Default::default() }
    }

    pub fn event(mut self, e: &'static str) -> Self {
        self.events.push(e);
        self
    }

    pub fn events(mut self, es: &[&'static str]) -> Self {
        for e in es {
            if !self.events.contains(e) {
                self.events.push(e);
            }
        }
        self
    }

    pub fn state(mut self, s: &State) -> Self {
        self.state = Some(state_name(s));
        self
    }

    /// A column, by the name its header shows.
    pub fn var(mut self, name: &str) -> Self {
        self.var = Some(display_text(name));
        self
    }

    /// Where an arrow points (its `var` is where it starts).
    pub fn to(mut self, name: &str) -> Self {
        self.to = Some(display_text(name));
        self
    }

    pub fn row(self, r: usize) -> Self {
        self.rows(r, r)
    }

    pub fn rows(mut self, a: usize, b: usize) -> Self {
        self.rows = Some((a.min(b), a.max(b)));
        self
    }

    pub fn drop(mut self, d: bool) -> Self {
        self.drop = d;
        self
    }

    pub fn part(mut self, p: &'static str) -> Self {
        self.part = Some(p);
        self
    }

    pub fn at(mut self, key: String) -> Self {
        self.at = Some(key);
        self
    }

    /// The internal attributes, with a leading space, for `finalize`.
    pub fn attrs(&self) -> String {
        let mut s = format!(" data-rv-kind=\"{}\"", self.kind);
        if !self.events.is_empty() {
            s.push_str(&format!(" data-rv-event=\"{}\"", self.events.join(" ")));
        }
        if let Some(st) = &self.state {
            s.push_str(&format!(" data-rv-state=\"{}\"", st));
        }
        if let Some(v) = &self.var {
            s.push_str(&format!(" data-rv-var=\"{}\"", escape(v)));
        }
        if let Some(v) = &self.to {
            s.push_str(&format!(" data-rv-to=\"{}\"", escape(v)));
        }
        if let Some((a, b)) = self.rows {
            s.push_str(&format!(" data-rv-row=\"{} {}\"", a, b));
        }
        if let Some(p) = self.part {
            s.push_str(&format!(" data-rv-part=\"{}\"", p));
        }
        if self.drop {
            s.push_str(" data-rv-drop=\"true\"");
        }
        if let Some(k) = &self.at {
            s.push_str(&format!(" data-rv-at=\"{}\"", escape(k)));
        }
        s
    }
}

/// XML-escapes an attribute value.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// `data-rv-event` of an event dot: the `Event` variant, in snake_case.
/// Exhaustive on purpose: a new variant needs a name here (and in
/// `docs/element-ids.md`).
pub fn event_name(e: &Event) -> &'static str {
    match e {
        Event::Move { break_alt: Some(BreakAlt { runtime: AltRuntime::Skipped, .. }), .. }
        | Event::Duplicate { break_alt: Some(BreakAlt { runtime: AltRuntime::Skipped, .. }), .. } => SKIPPED_EXIT,
        Event::Acquire { .. } => "acquire",
        Event::Duplicate { .. } => "duplicate",
        Event::Copy { .. } => "copy",
        Event::Move { .. } => MOVE,
        Event::Branch { .. } => "branch",
        Event::MutableLend { .. } => "mutable_lend",
        Event::MutableBorrow { .. } => "mutable_borrow",
        Event::MutableDie { .. } => "mutable_die",
        Event::MutableReacquire { .. } => "mutable_reacquire",
        Event::StaticLend { .. } => "static_lend",
        Event::StaticBorrow { .. } => "static_borrow",
        Event::StaticDie { .. } => "static_die",
        Event::StaticReacquire { .. } => "static_reacquire",
        Event::RefDie { .. } => "ref_die",
        Event::OwnerGoOutOfScope { .. } => "owner_go_out_of_scope",
        Event::RefGoOutOfScope { .. } => "ref_go_out_of_scope",
        Event::OwnerDropAtReassign { skipped: true, .. } => "skipped_assignment",
        Event::OwnerDropAtReassign { .. } => "owner_drop_at_reassign",
        Event::ManuallyDropByHand => "manually_drop_by_hand",
        Event::TemporaryDrop { .. } => "temporary_drop",
        Event::LoopIteratorDrop { .. } => "loop_iterator_drop",
        Event::LoopMaybeInit { .. } => "loop_maybe_init",
        Event::LoopExitKept { .. } => "loop_exit_kept",
        Event::LoopExitMove { at: MaybeMovedAt::LoopPartial, .. } => "loop_partial_move",
        Event::LoopExitMove { .. } => "loop_exit_move",
        Event::OwnerDropIfTryReturns { .. } => "owner_drop_if_try_returns",
        Event::OwnerDropUnlessReturned { is_break: true } => "owner_drop_unless_break_value",
        Event::OwnerDropUnlessReturned { .. } => "owner_drop_unless_returned",
        Event::TupleTempDrop { .. } => "tuple_temp_drop",
        Event::OrAltMove { .. } => "or_alt_move",
        Event::OrAltLend { .. } => "or_alt_lend",
        Event::OrAltBorrow { .. } => "or_alt_borrow",
        Event::ChainFailAssign { .. } => "chain_fail_assign",
        Event::ChainFailUse { .. } => "chain_fail_use",
        Event::TiedParamNote { .. } => "tied_param_note",
        Event::ReslicedInPlace { .. } => "resliced_in_place",
        Event::AssignmentJoin { .. } => BRANCH_JOIN,
        Event::DeadElseNote => "dead_else_note",
        Event::PassRestDrop { .. } => "pass_rest_drop",
        Event::LentWithParent { .. } => "lent_with_parent",
        Event::DeadLoanNote { .. } => "dead_loan_note",
        Event::InitRefParam { .. } => "init_ref_param",
        Event::ClosureCall { .. } => "closure_call",
    }
}

/// The join dot of a conditional (`Event::Branch`'s merge point); its
/// split dot is `branch`.
pub const BRANCH_JOIN: &str = "branch_join";

pub const MOVE: &str = "move";

/// A note on a transfer belonging to an exit the runtime skips.
pub const SKIPPED_EXIT: &str = "skipped_exit";

/// `data-rv-event` of an arrow, call icon or function logo: the
/// `ExternalEvent` variant, in snake_case.
pub fn external_event_name(e: &ExternalEvent) -> &'static str {
    match e {
        ExternalEvent::Move { break_alt, .. } | ExternalEvent::Copy { break_alt, .. }
        | ExternalEvent::StaticBorrow { break_alt, .. } | ExternalEvent::MutableBorrow { break_alt, .. }
            if break_alt.as_ref().is_some_and(|alt| alt.runtime == AltRuntime::Skipped) => SKIPPED_EXIT,
        ExternalEvent::Bind { .. } => "bind",
        ExternalEvent::Copy { .. } => "copy",
        ExternalEvent::Move { .. } => MOVE,
        ExternalEvent::StaticBorrow { .. } => "static_borrow",
        ExternalEvent::MutableBorrow { .. } => "mutable_borrow",
        ExternalEvent::StaticDie { .. } => "static_die",
        ExternalEvent::MutableDie { .. } => "mutable_die",
        ExternalEvent::RefDie { .. } => "ref_die",
        ExternalEvent::PassByStaticReference { .. } => "pass_by_static_reference",
        ExternalEvent::PassByMutableReference { .. } => "pass_by_mutable_reference",
        ExternalEvent::GoOutOfScope { .. } => "go_out_of_scope",
        ExternalEvent::OwnerDropAtReassign { skipped: true, .. } => "skipped_assignment",
        ExternalEvent::OwnerDropAtReassign { .. } => "owner_drop_at_reassign",
        ExternalEvent::ManuallyDropByHand { .. } => "manually_drop_by_hand",
        ExternalEvent::TemporaryDrop { .. } => "temporary_drop",
        ExternalEvent::LoopIteratorDrop { .. } => "loop_iterator_drop",
        ExternalEvent::LoopMaybeInit { .. } => "loop_maybe_init",
        ExternalEvent::LoopExitKept { .. } => "loop_exit_kept",
        ExternalEvent::LoopExitMove { at: MaybeMovedAt::LoopPartial, .. } => "loop_partial_move",
        ExternalEvent::LoopExitMove { .. } => "loop_exit_move",
        ExternalEvent::OwnerDropIfTryReturns { .. } => "owner_drop_if_try_returns",
        ExternalEvent::OwnerDropUnlessReturned { is_break: true, .. } => "owner_drop_unless_break_value",
        ExternalEvent::OwnerDropUnlessReturned { .. } => "owner_drop_unless_returned",
        ExternalEvent::TupleTempDrop { .. } => "tuple_temp_drop",
        ExternalEvent::OrAltMove { .. } => "or_alt_move",
        ExternalEvent::OrAltBorrow { .. } => "or_alt_borrow",
        ExternalEvent::ChainFailAssign { .. } => "chain_fail_assign",
        ExternalEvent::ChainFailUse { .. } => "chain_fail_use",
        ExternalEvent::TiedParamNote { .. } => "tied_param_note",
        ExternalEvent::ReslicedInPlace { .. } => "resliced_in_place",
        ExternalEvent::AssignmentJoin { .. } => BRANCH_JOIN,
        ExternalEvent::DeadElseNote { .. } => "dead_else_note",
        ExternalEvent::PassRestDrop { .. } => "pass_rest_drop",
        ExternalEvent::DeadLoanNote { .. } => "dead_loan_note",
        ExternalEvent::InitRefParam { .. } => "init_ref_param",
        ExternalEvent::ClosureCall { .. } => "closure_call",
        ExternalEvent::Branch { .. } => "branch",
    }
}

/// `data-rv-state` of a column segment: the `State` variant in
/// snake_case, then `_gray` (a conditional's arm that isn't the one
/// running on these rows, drawn dashed) or `_uninit` (no value yet).
pub fn state_name(s: &State) -> String {
    let (name, ls) = match s {
        State::OutOfScope => ("out_of_scope", None),
        State::ResourceMoved { .. } => ("resource_moved", None),
        State::FullPrivilege { s } => ("full_privilege", Some(s)),
        State::PartialPrivilege { s } => ("partial_privilege", Some(s)),
        State::RevokedPrivilege { .. } => ("revoked_privilege", None),
        State::ManuallyTaken { wrapper } => return match wrapper.as_ref() {
            State::PartialPrivilege { .. } => "manually_taken_shared",
            State::RevokedPrivilege { .. } => "manually_taken_mut",
            _ => "manually_taken",
        }.to_owned(),
        State::Invalid => ("invalid", None),
    };
    match ls {
        Some(LineState::ConditionalRemainder) => "conditional_remainder".to_owned(),
        Some(LineState::Gray) => format!("{}_gray", name),
        Some(LineState::Uninit) => format!("{}_uninit", name),
        Some(LineState::Full) | None => name.to_owned(),
    }
}

/// The column an arrow end is drawn on: the variable's name, `caller`
/// for the function's caller, `temp` for a value with no column.
pub fn party_name(r: &ResourceTy) -> String {
    match r {
        ResourceTy::Value(rap) | ResourceTy::Deref(rap)
        | ResourceTy::Index(rap, _, _) | ResourceTy::DerefField(rap, _, _, _) => rap_name(rap),
        ResourceTy::Caller => "caller".to_owned(),
        ResourceTy::Anonymous | ResourceTy::Temporary { .. } => "temp".to_owned(),
    }
}

pub fn rap_name(rap: &ResourceAccessPoint) -> String {
    display_text(rap.name())
}

/// The attributes of the code panel's line `line` (as its gutter
/// numbers it).
pub fn code_line_attrs(line: usize) -> String {
    format!(" data-rv-id=\"code:L{line}\" data-rv-kind=\"{CODE_LINE}\" data-rv-line=\"{line}\"")
}

/// Rewrites the internal attributes `Rv::attrs` wrote into the final
/// ones, in place. `row_lines[r]` is the line the code panel numbers
/// visual row `r` with, `None` for a row it adds (blank rows around a
/// conditional or before a function, rows arrows are stacked on).
///
/// Each element gets `data-rv-id`:
///
///   * `hdr:<var>` for a column header;
///   * else `<var>[><to>]@L<line>[-<end>]:<kind>[:<detail>]`, the detail
///     being the events joined by `+`, or the state.
///
/// When several elements would get the same id, the second gets `#2`
/// appended, the third `#3`, in document order (so the same program
/// gives the same ids every time). A node that is part of an element
/// (`data-rv-part`) gets `/<part>` after that.
pub fn finalize(svg: &str, row_lines: &[Option<usize>]) -> String {
    const KEY: &str = " data-rv-kind=\"";
    let mut out = String::with_capacity(svg.len() + svg.len() / 4);
    let mut rest = svg;
    // base id -> the elements (by `at` key) seen with it, in order.
    let mut seen: HashMap<String, Vec<String>> = HashMap::new();
    let mut node = 0usize;
    while let Some(i) = rest.find(KEY) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // Parse the run of ` data-rv-NAME="VALUE"` attributes.
        let mut attrs: Vec<(&str, &str)> = Vec::new();
        while let Some(after) = rest.strip_prefix(" data-rv-") {
            let Some(eq) = after.find("=\"") else { break };
            let name = &after[..eq];
            let val_start = &after[eq + 2..];
            let Some(end) = val_start.find('"') else { break };
            attrs.push((name, &val_start[..end]));
            rest = &val_start[end + 1..];
        }
        let get = |n: &str| attrs.iter().find(|(k, _)| *k == n).map(|(_, v)| *v);
        let kind = get("kind").unwrap_or("");
        let lines = get("row").and_then(|r| {
            let mut it = r.split(' ').filter_map(|x| x.parse::<usize>().ok());
            let (a, b) = (it.next()?, it.next()?);
            Some((line_of(a, row_lines)?, line_of(b, row_lines)?))
        });
        let detail = get("event").map(|e| e.replace(' ', "+")).or_else(|| get("state").map(str::to_owned));
        let base = if kind == HEADER {
            format!("hdr:{}", get("var").unwrap_or(""))
        } else {
            let mut b = get("var").unwrap_or("").to_owned();
            if let Some(to) = get("to") {
                b.push('>');
                b.push_str(to);
            }
            if let Some((a, e)) = lines {
                b.push_str(&format!("@L{}", a));
                if e != a {
                    b.push_str(&format!("-{}", e));
                }
            }
            b.push(':');
            b.push_str(kind);
            if let Some(d) = &detail {
                b.push(':');
                b.push_str(d);
            }
            b
        };
        // Ids are tokens: no whitespace, and at most 128 characters with
        // the suffixes below (a dot with many events, a long name).
        let mut base: String = base.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
        if base.chars().count() > BASE_MAX {
            base = base.chars().take(BASE_MAX).collect();
            // Not in the middle of an entity (`&lt;`).
            if let Some(amp) = base.rfind('&').filter(|&i| !base[i..].contains(';')) {
                base.truncate(amp);
            }
        }
        let at = get("at").map(str::to_owned).unwrap_or_else(|| format!("#node{}", node));
        node += 1;
        let elems = seen.entry(base.clone()).or_default();
        let n = match elems.iter().position(|k| *k == at) {
            Some(p) => p + 1,
            None => {
                elems.push(at);
                elems.len()
            }
        };
        let mut id = base;
        if n > 1 {
            id.push_str(&format!("#{}", n));
        }
        if let Some(p) = get("part") {
            id.push('/');
            id.push_str(p);
        }
        out.push_str(&format!(" data-rv-id=\"{}\" data-rv-kind=\"{}\"", id, kind));
        for name in ["event", "state", "var", "to"] {
            if let Some(v) = get(name) {
                out.push_str(&format!(" data-rv-{}=\"{}\"", name, v));
            }
        }
        if let Some((a, e)) = lines {
            out.push_str(&format!(" data-rv-line=\"{}\"", a));
            if e != a {
                out.push_str(&format!(" data-rv-line-end=\"{}\"", e));
            }
        }
        for name in ["part", "drop"] {
            if let Some(v) = get(name) {
                out.push_str(&format!(" data-rv-{}=\"{}\"", name, v));
            }
        }
    }
    out.push_str(rest);
    out
}

/// ` data-rv-invalid="…"` for the timeline's root `<svg>`, or `""`:
/// every stretch of a column whose state is `State::Invalid`, as
/// `var@L<line>[-<end>]`, sorted and space-separated. `calc_state` falls
/// through to Invalid when it has no rule for an event in the state the
/// column is in; such a stretch draws nothing, so the column silently
/// vanishes (#1122). Tests read this attribute to catch it
/// (`rustviz-lib/tests/invariants.rs`); it changes nothing drawn.
pub fn invalid_states_attr(vd: &VisualizationData, row_lines: &[Option<usize>]) -> String {
    fn walk(name: &str, states: &[(usize, usize, State)], history: &[(usize, Event)],
            row_lines: &[Option<usize>], out: &mut Vec<String>) {
        for (a, b, st) in states {
            if matches!(st, State::Invalid) {
                let (Some(a), Some(b)) = (line_of(*a, row_lines), line_of(*b, row_lines)) else { continue };
                out.push(if a == b { format!("{}@L{}", name, a) } else { format!("{}@L{}-{}", name, a, b) });
            }
        }
        for (_, e) in history {
            if let Event::Branch { branch_history, .. } = e {
                for b in branch_history {
                    walk(name, &b.states, &b.e_data, row_lines, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for t in vd.timelines.values() {
        if matches!(t.resource_access_point, ResourceAccessPoint::Function(_)) {
            continue;
        }
        let name = escape(&display_text(t.resource_access_point.name()));
        let name: String = name.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
        walk(&name, &t.states, &t.history, row_lines, &mut out);
    }
    out.sort();
    out.dedup();
    if out.is_empty() {
        String::new()
    } else {
        format!(" data-rv-invalid=\"{}\"", out.join(" "))
    }
}

/// Longest `data-rv-id` before the `#n` and `/part` suffixes, so that an
/// id stays within 128 characters (a telemetry event's `element_id`).
/// Longer ones are cut; making ids unique comes after, so a cut id is
/// still unique.
const BASE_MAX: usize = 112;

/// The code panel's line number for visual row `r`: its own, or for a
/// row the renderer added, the nearest numbered row above it (below it,
/// above the first line).
pub fn line_of(r: usize, row_lines: &[Option<usize>]) -> Option<usize> {
    if row_lines.is_empty() {
        return None;
    }
    let r = r.min(row_lines.len() - 1);
    row_lines[..=r].iter().rev().find_map(|l| *l)
        .or_else(|| row_lines[r..].iter().find_map(|l| *l))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_built_and_made_unique() {
        let rows = vec![None, Some(1), Some(2), None, Some(3)];
        let svg = format!(
            "<circle{}/><circle{}/><g{}/><line{}/><polygon{}/><polygon{}/><text{}/>",
            Rv::new(DOT).event("move").var("s").row(2).attrs(),
            Rv::new(DOT).event("move").var("s").row(2).attrs(),
            Rv::new(ARROW).event("move").var("s").to("t<u>").row(3).attrs(),
            Rv::new(BRANCH_LINE).state(&State::FullPrivilege { s: LineState::Gray }).var("s").rows(1, 4).part("line").at("a".into()).attrs(),
            Rv::new(BRANCH_LINE).state(&State::FullPrivilege { s: LineState::Gray }).var("s").rows(1, 4).at("a".into()).attrs(),
            Rv::new(BRANCH_LINE).state(&State::FullPrivilege { s: LineState::Gray }).var("s").rows(1, 4).at("b".into()).attrs(),
            Rv::new(HEADER).var("s").attrs(),
        );
        let out = finalize(&svg, &rows);
        let ids: Vec<&str> = out.split("data-rv-id=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
        assert_eq!(ids, [
            "s@L2:dot:move",
            "s@L2:dot:move#2",
            "s>t&lt;u&gt;@L2:arrow:move",
            "s@L1-3:branch_line:full_privilege_gray/line",
            "s@L1-3:branch_line:full_privilege_gray",
            "s@L1-3:branch_line:full_privilege_gray#2",
            "hdr:s",
        ]);
        assert!(out.contains(" data-rv-line=\"1\" data-rv-line-end=\"3\" data-rv-part=\"line\""));
        assert!(!out.contains("data-rv-row") && !out.contains("data-rv-at"));
    }

    #[test]
    fn long_ids_are_cut_and_stay_unique() {
        let rows = vec![None, Some(1)];
        let many = ["static_lend", "static_reacquire", "mutable_lend", "mutable_reacquire",
                    "owner_drop_at_reassign", "acquire", "pass_by_mutable_reference"];
        let dot = Rv::new(DOT).events(&many).var("a very_long_name<T>").row(1).attrs();
        let out = finalize(&format!("<circle{}/><circle{}/>", dot, dot), &rows);
        let ids: Vec<&str> = out.split("data-rv-id=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        for id in &ids {
            assert!(id.chars().count() <= 128 && !id.contains(' '), "{:?}", id);
        }
        assert!(ids[0].starts_with("a_very_long_name&lt;T&gt;@L1:dot:static_lend+"));
        // The full list is still in data-rv-event.
        assert!(out.contains(&format!("data-rv-event=\"{}\"", many.join(" "))));
    }
}
