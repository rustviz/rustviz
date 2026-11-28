//! Several condition edges can enter one drawn arm. Find the loan boundary
//! on each incoming history before attributing the end to a particular slot.
use super::{Boundary, ExprVisitor, Path};
use crate::facts::{real_successor_blocks, Ev, Fact, SourceArm, SwitchEdge};
use crate::svg_generator::data::{AltRuntime, ExternalEvent, ResourceTy};
use crate::svg_generator::hover_messages::{break_alt_if_suffix, styled_name_html};
use rustc_middle::mir::{Body, Location, Operand, Rvalue, StatementKind, TerminatorKind};
use std::collections::{BTreeMap, BTreeSet};

/// A boolean temporary sometimes holds a literal (the final expression of
/// a condition block). Borrowck MIR still contains both switch targets.
fn feasible(body: &Body<'_>, edge: SwitchEdge) -> bool {
  let block = &body[edge.switch.block];
  let TerminatorKind::SwitchInt { discr, targets } = &block.terminator().kind else { return true };
  let value = match discr {
    Operand::Constant(c) => c.const_.try_to_bool(),
    _ => discr.place().and_then(|p| {
      let local = p.as_local()?;
      for st in block.statements.iter().rev() {
        match &st.kind {
          StatementKind::StorageDead(l) | StatementKind::StorageLive(l) if *l != local => {}
          StatementKind::Assign(pair) if pair.0 == p => {
            let Rvalue::Use(Operand::Constant(c)) = &pair.1 else { return None };
            return c.const_.try_to_bool();
          }
          _ => return None,
        }
      }
      None
    }),
  };
  value.is_none_or(|v| targets.target_for_value(u128::from(v)) == edge.successor)
}

pub(super) fn borrow_kind(kind: &str) -> Option<&'static str> {
  match kind {
    "mut" => Some("mutable"),
    "shared" => Some("shared"),
    _ => None,
  }
}

pub(super) fn call_title(holder: &str, lender: &str, kind: &str, exact: bool) -> String {
  let mut text = if exact {
    format!("{holder}'s {kind} borrow of {lender} ends")
  } else {
    format!("{holder} may have referred to {lender}. The {kind} borrow of {lender} ends")
  };
  text.push_str(&format!(". This borrow no longer restricts access to {lender}"));
  if exact {
    // The teaching clause the ordinary end title carries: a reference
    // ending is not the lender being moved.
    text.push_str(". Ending the borrow doesn't move the value");
  }
  text
}

pub(super) fn release_title(holder: Option<&str>, lender: &str, kind: &str) -> String {
  let subject = match holder {
    Some(holder) => format!("{}'s {kind} borrow", styled_name_html(holder)),
    None => format!("The {kind} borrow"),
  };
  format!("{subject} of {lender} ends. This borrow no longer restricts access to {lender}.")
}

pub(super) struct ForkCases {
  pub text: String,
  pub calls: BTreeSet<Location>,
  pub common_holder: Option<rustc_middle::mir::Local>,
}

impl ExprVisitor<'_, '_> {
  pub(super) fn edge_release_title(
    &self, facts: &crate::facts::BodyFacts, event: &ExternalEvent, ends: &[Boundary<'_>],
  ) -> Option<(String, Option<String>)> {
    let first = ends.first()?;
    if !matches!(event, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. })
      || !ends.iter().all(|end| end.release && end.loan == first.loan)
    {
      return None;
    }
    let loan = facts.loans.iter().find(|l| l.loan == first.loan)?;
    let kind = borrow_kind(loan.kind)?;
    let lender = self.fork_lender(loan)?;
    let (from, to) = super::endpoints(event);
    let slot = from.extract_rap().and_then(|r| Some((r, r.ref_local()?)));
    let holder = slot.filter(|&(_, local)| {
      ends.iter().all(|end| end.fact.exact_end_places.iter().any(|p| p.local == local.index() && p.projection.is_empty()))
    });
    let payload = slot.and_then(|(r, local)| {
      let (container_local, container, projected_kind) = crate::facts::call_wording::payload_borrow(
        self.tcx, &self.bwf.body, local, &self.raps.bindings,
      )?;
      let mut inits = facts.rows.iter().flat_map(|row| &row.facts)
        .filter(|ev| ev.place_local == Some(local) && matches!(ev.fact, Fact::Init { .. }));
      let init = inits.next().filter(|_| inits.next().is_none())?;
      (Some(container_local) == loan.lender_local
        && ends.iter().all(|end| super::dominates(&self.bwf.body, init.location, end.fact.occurrence.at)
          && end.fact.end_places.iter().any(|p| p.local == local.index() && p.projection.is_empty())))
        .then(|| format!("{} holds a {projected_kind} reference to the payload of {}. ",
        styled_name_html(r.name()), styled_name_html(&container)))
    });
    let named_holder = holder.filter(|_| payload.is_none()).map(|(r, _)| r.name().as_str());
    let mut title = payload.unwrap_or_default();
    title.push_str(&release_title(named_holder, &lender, kind));

    let restored = to.extract_rap().filter(|rap| {
      loan.lender_local.is_some_and(|local| self.raps.place(local, &loan.lender).is_some_and(|slot| &slot.rap == *rap))
        && ends.iter().all(|end| facts.usable_after_release.contains(&(loan.loan, end.fact.occurrence)))
    }).map(|rap| {
      let noun = if rap.drops_nothing() { "value" } else { "resource" };
      let adverb = if kind == "mutable" { "mutably" } else { "immutably" };
      format!("{lender}'s {noun} is no longer {adverb} borrowed")
    });
    Some((title, restored))
  }

  /// Match each drawn end to the normal return edge of the argument's
  /// MIR call. The bypass and return can share both endpoints and loan ID.
  pub(crate) fn describe_joined_argument_ends(&mut self) {
    let Some(facts) = self.body_facts else { return };
    if self.argument_slots.is_empty() { return; }
    let body = &self.bwf.body;
    let switches = self.branch_switches();
    let mut groups = vec![(Path::new(), &*self.preprocessed_events)];
    while let Some((path, events)) = groups.pop() {
      let mut calls = BTreeMap::<_, Vec<_>>::new();
      for (_, event) in events {
        match event {
          ExternalEvent::Branch { id, branches, .. } => {
            groups.extend(branches.iter().enumerate().map(|(i, arm)| {
              ([path.as_slice(), &[(*id, i)]].concat(), &arm.e_data)
            }));
          }
          ExternalEvent::PassByStaticReference { id, from, to, break_alt: Some(alt), .. }
          | ExternalEvent::PassByMutableReference { id, from, to, break_alt: Some(alt), .. }
            if alt.runtime == AltRuntime::Possible && alt.brk.is_some() =>
          {
            let Some(&slot) = self.argument_slots.get(id) else { continue };
            let Some(holder) = from.extract_rap().and_then(|r| r.ref_local()) else { continue };
            let Some(reborrow) = facts.loans.iter().find(|l| Some(l.loan) == alt.reborrow && l.lender_local == Some(holder)) else { continue };
            calls.entry(slot).or_default().push((holder, to, alt, reborrow));
          }
          _ => {}
        }
      }
      for (slot @ (at, _), operands) in calls {
        let Some(sources) = facts.joined_argument_loans.get(&slot) else { continue };
        let TerminatorKind::Call { target: Some(target), .. } = body[at.block].terminator().kind else { continue };
        let successor = Some(target.start_location());
        let ends = |loan| self.reference_effects.ends(loan)
          .filter(|end| end.release && end.fact.drop_route == crate::facts::DropRoute::Normal);
        let at_call = |end: &Ev| end.occurrence.at == at && end.occurrence.successor == successor;
        for &(holder, func, alt, reborrow) in &operands {
          if !sources.contains(&reborrow.loan) || !ends(reborrow.loan).any(|e| at_call(e.fact)) { continue; }
          let &[parent] = reborrow.made_through.as_slice() else { continue };
          let Some(loan) = facts.loans.iter().find(|l| l.loan == parent) else { continue };
          let Some(kind) = borrow_kind(loan.kind) else { continue };
          let Some(lender_local) = self.bwf.borrow_set.location_map().get_index(parent).and_then(|(_, data)| data.borrowed_place().as_local()) else { continue };
          let mut slots = events.iter().filter_map(|(row, event)| {
            let (ExternalEvent::StaticDie { from: ResourceTy::Value(from), to: ResourceTy::Value(to), still_borrowed_by, id, .. }
              | ExternalEvent::MutableDie { from: ResourceTy::Value(from), to: ResourceTy::Value(to), still_borrowed_by, id, .. }) = event else { return None };
            if from.ref_local() != Some(holder) || to.is_member() || to.owner_local() != Some(lender_local)
              || !still_borrowed_by.is_empty() || self.end_loans.get(id).is_none_or(|ids| ids != &[parent]) {
              return None;
            }
            let mut boundaries = ends(parent).filter(|e| {
              if let Some(occurrence) = self.row_layout.arm_end_occurrences.get(id) {
                return e.fact.occurrence == *occurrence;
              }
              let Fact::BorrowEnd { to_line, .. } = e.fact.fact else { return false };
              let (site, _) = self.normal_fork_site(facts, &switches, e.row, e.fact, to_line);
              site.matches(self.preprocessed_events, &path, *row, *id)
            });
            let boundary = boundaries.next()?.fact;
            (boundaries.all(|e| e.fact.occurrence == boundary.occurrence) && at_call(boundary))
              .then_some((*id, from, to, boundary))
          });
          let Some((id, from, to, boundary)) = slots.next() else { continue };
          if slots.next().is_some() { continue; }
          let selected = break_alt_if_suffix(alt);
          let func = styled_name_html(&func.name());
          let holder_name = styled_name_html(from.name());
          let what = if to.drops_nothing() { "value" } else { "resource" };
          let exact = facts.exact_reborrow_parents.get(&reborrow.loan) == Some(&parent);
          let lender = styled_name_html(to.name());
          let mut title = if exact {
            format!("{holder_name}'s {kind} borrow of {lender} ends when {func} returns{selected}.")
          } else {
            format!("If {holder_name} held this {kind} borrow and supplied the argument, the borrow ends when {func} returns and no longer restricts access to the borrowed place.")
          };
          let others = || sources.iter().copied().filter(|&id| id != reborrow.loan);
          // Account for unnamed alternatives too, such as a direct borrow
          // of an owner. Every alternate source needs a proved bypass.
          let has_bypass = |id| facts.loans.iter().find(|l| l.loan == id).is_some_and(|other| {
            other.issuance_path.iter().any(|edge| reborrow.issuance_path.iter().any(|chosen|
              chosen.switch == edge.switch && chosen.successor != edge.successor)
              && ends(parent).any(|e| e.fact.occurrence.at == edge.switch
                && e.fact.occurrence.successor == Some(edge.successor.start_location())
                && e.fact.exact_end_places.iter().any(|p| p.local == holder.index() && p.projection.is_empty())))
          });
          if exact && others().all(has_bypass) {
            let other_alt = (sources.len() == 2).then(|| {
              operands.iter().find(|(_, _, _, other)| other.loan != reborrow.loan && sources.contains(&other.loan))
                .map(|(_, _, alt, _)| *alt)
            }).flatten();
            if let Some(other_alt) = other_alt.filter(|other| other.brk != alt.brk) {
              title.push_str(&format!(" The borrow has already ended{}.", break_alt_if_suffix(other_alt)));
            } else {
              title.push_str(" This borrow has already ended on paths that pass a different reference.");
            }
          }
          if exact {
            title.push_str(&format!(" This borrow no longer restricts access to {lender}."));
          }
          title.push_str(&format!(" Ending the borrow doesn't move the {what}"));
          let restored = (exact && facts.usable_after_release.contains(&(parent, boundary.occurrence))).then(|| {
            let kind = if kind == "mutable" { "mutably" } else { "immutably" };
            format!("{lender}'s {what} is no longer {kind} borrowed")
          });
          self.row_layout.joined_end_titles.insert(id, (title, restored));
        }
      }
    }
  }

  /// Use a source column or the fact layer's projection wording. Some MIR
  /// downcasts have no source-level name for the borrowed payload.
  pub(super) fn fork_lender(&self, loan: &crate::facts::LoanInfo) -> Option<String> {
    if let Some(place) = &loan.projected {
      return Some(crate::svg_generator::hover_messages::projected_place_html(place));
    }
    let local = loan.lender_local?;
    if self.raps.place(local, &loan.lender).is_some() {
      return Some(styled_name_html(&loan.lender));
    }
    let place = self.bwf.borrow_set.location_map().get_index(loan.loan)?.1.borrowed_place();
    use rustc_middle::mir::ProjectionElem;
    if let [ProjectionElem::Downcast(_, variant), ProjectionElem::Field(_, ty)] = place.projection.as_slice() {
      let def = self.bwf.body.local_decls[place.local].ty.ty_adt_def()?;
      let variant = def.variant(*variant);
      if variant.fields.len() == 1 {
        let name = styled_name_html(self.raps.bindings.locals.get(&place.local)?);
        let variant = styled_name_html(&variant.name.to_string());
        let payload = ty.ty_adt_def().map(|adt| styled_name_html(&self.tcx.item_name(adt.did()).to_string()))
          .unwrap_or_else(|| "payload".to_owned());
        return Some(format!("the {payload} inside {name}'s {variant} variant"));
      }
    }
    if place.projection.as_slice() == [ProjectionElem::Deref] {
      return Some(format!("what {} points to", styled_name_html(self.raps.bindings.locals.get(&place.local)?)));
    }
    None
  }

  pub(super) fn fork_end_cases(&self, arm: SourceArm, loan: usize) -> Option<ForkCases> {
    let facts = self.body_facts?;
    let body = &self.bwf.body;
    let mut entries = facts
      .switches
      .iter()
      .filter(|s| s.branch == Some(arm.branch) && s.arm == Some(arm.index))
      .map(|s| s.edge)
      .filter(|e| feasible(body, *e))
      .peekable();
    let first = entries.next()?;
    entries.peek()?;
    let info = facts.loans.iter().find(|l| l.loan == loan)?;
    let issue = info.issue?;
    let ends: BTreeMap<_, _> = facts
      .rows
      .iter()
      .flat_map(|r| &r.facts)
      .filter(|e| matches!(e.fact, Fact::BorrowEnd { loan: l, .. } if l == loan))
      .filter_map(|e| Some(((e.occurrence.at, e.occurrence.successor?), e)))
      .collect();
    let snippet = |sp: rustc_span::Span| {
      (!sp.from_expansion()).then(|| self.tcx.sess.source_map().span_to_snippet(sp).ok()).flatten()
    };
    let mut cases = Vec::new();
    let mut holders = BTreeSet::new();
    let mut boundaries = BTreeSet::new();
    let mut calls = BTreeSet::new();
    for entry in std::iter::once(first).chain(entries) {
      let edge = (entry.switch, entry.successor.start_location());
      let end = match ends.get(&edge) {
        Some(&end) => end,
        None => preceding_end(body, &ends, issue, entry.switch)?,
      };
      let [holder] = end.end_places.as_slice() else { return None };
      let local = rustc_middle::mir::Local::from_usize(holder.local);
      if !holder.projection.is_empty() || !body.local_decls[local].ty.is_ref() {
        return None;
      }
      holders.insert(local);
      boundaries.insert((end.occurrence.at, end.occurrence.successor?));
      let name = styled_name_html(&holder.name);
      let test = styled_name_html(&snippet(body.source_info(entry.switch).span)?);
      let term = body[end.location.block].terminator();
      let kind = borrow_kind(info.kind)?;
      let lender = self.fork_lender(info)?;
      let call = match term.kind {
        TerminatorKind::Call { fn_span, target: Some(to), .. }
          if end.location == body.terminator_loc(end.location.block)
            && end.occurrence.successor == Some(to.start_location()) =>
        {
          snippet(fn_span)
        }
        _ => None,
      };
      let exact = end.exact_end_places.contains(holder);
      let text = if !exact {
        let mut text = format!("When the test of {test} leads here, {name} may have referred to {lender}");
        if let Some(call) = call {
          calls.insert(end.location);
          if end.lender_writable {
            text.push_str(&format!(
              ". The {kind} borrow of {lender} ended when {} returned. This borrow no longer restricts access to {lender}",
              styled_name_html(&call)
            ));
          }
        }
        text
      } else if edge == (end.occurrence.at, end.occurrence.successor?) {
        format!("When the test of {test} leads here, {name}'s {kind} borrow of {lender} ends")
      } else if let Some(call) = call {
        calls.insert(end.location);
        format!(
          "When the test of {test} leads here, {name}'s {kind} borrow of {lender} already ended when {} returned",
          styled_name_html(&call)
        )
      } else {
        format!("When the test of {test} leads here, {name}'s {kind} borrow of {lender} has already ended")
      };
      cases.push(text);
    }
    (boundaries.len() > 1).then(|| ForkCases {
      text: cases.join(". "),
      calls,
      common_holder: (holders.len() == 1).then(|| *holders.first().unwrap()),
    })
  }

  /// Match the call's return edge to a single drawing slot. Several loans
  /// can end in the very same reference when its value came from a call.
  pub(super) fn name_fork_call_ends(&mut self, calls: &BTreeSet<Location>) {
    if calls.is_empty() {
      return;
    }
    let Some(facts) = self.body_facts else { return };
    let mut slots = Vec::new();
    super::templates_in(self.preprocessed_events, &mut Vec::new(), Some(&self.end_loans), &mut slots);
    for (_, ev, _) in slots {
      let (from, to) = super::endpoints(&ev);
      let (Some(holder), Some(lender)) = (from.extract_rap(), to.extract_rap()) else { continue };
      let Some(local) = holder.ref_local() else { continue };
      let [id] = self.end_loans[&ev.get_id()].as_slice() else { continue };
      let Some(loan) = facts.loans.iter().find(|l| l.loan == *id) else { continue };
      if !loan.lender_local.and_then(|l| self.raps.place(l, &loan.lender)).is_some_and(|r| &r.rap == lender) {
        continue;
      }
      let mut ends = facts.rows.iter().flat_map(|r| &r.facts).filter(|e| {
        matches!(e.fact, Fact::BorrowEnd { loan, .. } if loan == *id)
          && e.end_places.iter().any(|p| p.local == local.index() && p.projection.is_empty())
      });
      let Some(edge) = ends.next() else { continue };
      if ends.next().is_some() || !calls.contains(&edge.location) {
        continue;
      }
      let h = styled_name_html(holder.name());
      let Some(l) = self.fork_lender(loan) else { continue };
      let Some(kind) = borrow_kind(loan.kind) else { continue };
      let exact = edge.exact_end_places.iter().any(|p| p.local == local.index() && p.projection.is_empty());
      let text = call_title(&h, &l, kind, exact);
      self.row_layout.fork_arrow_titles.insert(ev.get_id(), text);
    }
  }
}

/// Stop at the first end walking backwards along *each* executable incoming
/// edge. Reaching issuance without a boundary means the loan is still live.
/// Crossing a backedge would mix iterations, which this query cannot label.
fn preceding_end<'a>(
  body: &Body<'_>,
  ends: &BTreeMap<(Location, Location), &'a Ev>,
  issue: Location,
  start: Location,
) -> Option<&'a Ev> {
  let mut pending = vec![start];
  let mut seen = BTreeSet::new();
  let mut found: Option<&Ev> = None;
  while let Some(at) = pending.pop() {
    if !seen.insert(at) {
      continue;
    }
    if at == issue {
      return None;
    }
    let preds = if at.statement_index > 0 {
      vec![Location { statement_index: at.statement_index - 1, ..at }]
    } else {
      body.basic_blocks.predecessors()[at.block]
        .iter()
        .copied()
        .filter(|&bb| real_successor_blocks(body, bb).contains(&at.block))
        .map(|bb| body.terminator_loc(bb))
        .filter(|&switch| feasible(body, SwitchEdge { switch, successor: at.block }))
        .collect()
    };
    if preds.is_empty() {
      return None;
    }
    for pred in preds {
      if let Some(&end) = ends.get(&(pred, at)) {
        if found.replace(end).is_some_and(|f| f.occurrence != end.occurrence) {
          return None;
        }
      } else if at.statement_index == 0 && body.basic_blocks.dominators().dominates(at.block, pred.block) {
        return None;
      } else {
        pending.push(pred);
      }
    }
  }
  found
}
