//! Project a normal borrow boundary onto the leg containing its MIR edge.
use super::{branch_at, endpoints, identify_end, insert, remove_except_reserved, templates_in, Path, Site};
use crate::expr_visitor::ExprVisitor;
use crate::facts::{EndPlace, Ev, Fact};
use crate::svg_generator::data::{BranchType, ExternalEvent, ResourceTy};
use crate::svg_generator::hover_messages::styled_name_html;
use rustc_middle::mir::{Local, Location, Operand, Rvalue, StatementKind, TerminatorKind};
use rustc_utils::BodyExt;
use std::collections::{BTreeMap, BTreeSet};

fn call_return(body: &rustc_middle::mir::Body<'_>, e: &Ev) -> bool {
  e.location == body.terminator_loc(e.location.block)
    && matches!(body[e.location.block].terminator().kind, TerminatorKind::Call { target: Some(t), .. }
      if e.occurrence.successor == Some(t.start_location()))
}

impl ExprVisitor<'_, '_> {
  /// A later operand in a let-chain is drawn on the success leg after it
  /// passes the earlier MIR switch, even when its own test ultimately fails.
  fn normal_fork_site(
    &self,
    facts: &crate::facts::BodyFacts,
    switches: &BTreeMap<usize, Vec<Location>>,
    row: usize,
    e: &Ev,
    next: usize,
  ) -> (Site, bool) {
    let mut s = Site { row, path: Path::new(), before: None };
    let entry = self.locate(self.preprocessed_events, switches, e.display_location, e, next, &mut s);
    let body = &self.bwf.body;
    if !entry {
      // Bindings and later condition operands are drawn on the success
      // leg. Only use that layout once a real earlier switch was passed.
      let id = body.location_to_hir_id(e.display_location);
      for arm in crate::facts::enclosing_arms(self.tcx, id, true).into_iter().rev() {
        let Some((_, ExternalEvent::Branch { id, branch_type, .. })) =
          super::events_at(self.preprocessed_events, &s.path).and_then(|es| {
            es.iter().find(|(_, ev)| {
              matches!(ev, ExternalEvent::Branch { branches, .. }
                if branches.get(arm.index).is_some_and(|b| b.source == Some(arm)))
            })
          })
        else {
          continue;
        };
        let passed = facts.switches.iter().any(|x| {
          x.branch == Some(arm.branch)
            && super::dominates(body, x.edge.switch, e.location)
            && x.edge.switch != e.location
        });
        if passed && !matches!(branch_type, BranchType::Loop(..)) {
          s.path.push((*id, arm.index));
          s.before = None;
        }
      }
    }
    (s, entry)
  }

  /// Identify an existing arm return only when it represents one compiler
  /// edge. Two uses of the same reference can end its loan on different
  /// edges, even on one source line. A holder or an arm alone cannot decide.
  fn fork_return_slots<'a>(
    &self, facts: &'a crate::facts::BodyFacts, switches: &BTreeMap<usize, Vec<Location>>,
    slots: &[(usize, ExternalEvent, Path)], loan: usize, lender: &ResourceTy,
  ) -> BTreeMap<usize, &'a Ev> {
    let mut represented = BTreeMap::new();
    for (row, slot, path) in slots {
      let (from, to) = endpoints(slot);
      if path.is_empty() || to != lender || !self.end_loans[&slot.get_id()].contains(&loan)
        || !matches!(slot, ExternalEvent::StaticDie { still_borrowed_by, .. }
          | ExternalEvent::MutableDie { still_borrowed_by, .. } if still_borrowed_by.is_empty()) {
        continue;
      }
      let Some(holder) = from.extract_rap().and_then(|r| r.ref_local()) else { continue };
      let mut edges = facts.rows.iter().flat_map(|r| r.facts.iter().map(move |e| (r.line, e))).filter_map(|(line, e)| {
        let Fact::BorrowEnd { loan: id, to_line, .. } = e.fact else { return None };
        if id != loan || e.drop_route != crate::facts::DropRoute::Normal
          || !matches!(e.end_places.as_slice(), [p] if p.projection.is_empty() && p.local == holder.index()) {
          return None;
        }
        let (site, _) = self.normal_fork_site(facts, switches, line, e, to_line);
        site.matches(self.preprocessed_events, path, *row, slot.get_id()).then_some(e)
      });
      if let Some(edge) = edges.next().filter(|first| edges.all(|e| e.occurrence == first.occurrence)) {
        represented.insert(slot.get_id(), edge);
      }
    }
    represented
  }

  /// Account for all other normal ends before deleting a fieldless return.
  /// If a single legacy template straddles a nested fork, split it only
  /// when each missing call return has a distinct MIR site in its scope.
  fn fieldless_sibling_returns<'a>(
    &self,
    facts: &'a crate::facts::BodyFacts,
    switches: &BTreeMap<usize, Vec<Location>>,
    slots: &[(usize, ExternalEvent, Path)],
    represented: &BTreeMap<usize, &'a Ev>,
    loan: usize,
    lender: &ResourceTy,
    (branch, arm): (usize, usize),
    empty: &Ev,
  ) -> Option<BTreeMap<usize, BTreeMap<Site, &'a Ev>>> {
    let sibling = |path: &Path| path.iter().any(|&(b, a)| b == branch && a != arm);
    let mut replacements = BTreeMap::<usize, BTreeMap<Site, &Ev>>::new();
    for (row, end) in facts.rows.iter().flat_map(|r| r.facts.iter().map(move |e| (r.line, e))) {
      let Fact::BorrowEnd { loan: id, to_line, .. } = end.fact else { continue };
      if id != loan || end.drop_route != crate::facts::DropRoute::Normal || empty.occurrence == end.occurrence {
        continue;
      }
      if slots.iter().any(|(_, slot, path)| sibling(path)
        && represented.get(&slot.get_id()).is_some_and(|e| e.occurrence == end.occurrence)) {
        continue;
      }

      let [place] = end.end_places.as_slice() else { return None };
      if !place.projection.is_empty() || !call_return(&self.bwf.body, end) { return None; }
      let (site, _) = self.normal_fork_site(facts, switches, row, end, to_line);
      let mut eligible = slots.iter().filter(|(_, slot, path)| {
        let (from, to) = endpoints(slot);
        sibling(path) && site.path.starts_with(path) && to == lender
          && self.end_loans[&slot.get_id()] == [loan]
          && from.extract_rap().and_then(|r| r.ref_local()) == Some(Local::from_usize(place.local))
          && matches!(slot, ExternalEvent::StaticDie { still_borrowed_by, .. }
            | ExternalEvent::MutableDie { still_borrowed_by, .. } if still_borrowed_by.is_empty())
      });
      let (_, slot, _) = eligible.next()?;
      if eligible.next().is_some() { return None; }
      let dests = replacements.entry(slot.get_id()).or_default();
      if dests.insert(site, end).is_some_and(|old| old.occurrence != end.occurrence) { return None; }
    }
    for (id, sites) in &mut replacements {
      if let Some(&old) = represented.get(id) {
        let (row, _, path) = slots.iter().find(|(_, slot, _)| slot.get_id() == *id)?;
        let site = Site { row: *row, path: path.clone(), before: None };
        if sites.insert(site, old).is_some_and(|e| e.occurrence != old.occurrence) { return None; }
      }
    }
    Some(replacements)
  }

  pub(crate) fn place_proven_fork_ends(&mut self) -> BTreeSet<(usize, crate::facts::SwitchEdge)> {
    let mut empty_payloads = BTreeSet::new();
    let Some(facts) = self.body_facts else { return empty_payloads };
    if facts.switches.is_empty() {
      return empty_payloads;
    }

    let switches = self.branch_switches();
    let mut slots = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), Some(&self.end_loans), &mut slots);
    let mut fieldless_lenders = BTreeMap::new();
    let mut removed = BTreeSet::new();
    let mut reserved = BTreeSet::new();
    let mut placed = BTreeSet::new();
    let mut calls = BTreeSet::new();
    let mut put = Vec::new();
    'slots: for (line, template, origin) in &slots {
      if !origin.is_empty()
        || matches!(template, ExternalEvent::StaticDie { exhaustion, .. } | ExternalEvent::MutableDie { exhaustion, .. } if !exhaustion.is_empty())
      {
        continue;
      }
      let (ResourceTy::Value(holder), to) = endpoints(template) else { continue };
      if to.extract_rap().is_some_and(|l| l.is_fn()) {
        continue;
      }
      if matches!(template, ExternalEvent::RefDie { .. }) {
        continue;
      }
      let &[loan] = self.end_loans[&template.get_id()].as_slice() else { continue };
      let belongs = |place: &EndPlace| {
        let local = Local::from_usize(place.local);
        self.raps.place(local, &place.name).is_some_and(|r| &r.rap == holder)
          || (!holder.is_member() && holder.owner_local() == Some(local))
      };
      let mut sites = BTreeMap::<(Site, bool), &Ev>::new();
      let mut fieldless_edges = Vec::new();
      let mut vacant_lenders = BTreeSet::new();
      let mut qualified = BTreeMap::new();
      for (row, e) in facts
        .rows
        .iter()
        .flat_map(|r| r.facts.iter().map(move |e| (r.line, e)))
        .chain(facts.inline_holder_ends.iter().filter(|(_, e)| !e.continuing_places.is_empty()).map(|(r, e)| (*r, e)))
      {
        let (id, next, hold) = match e.fact {
          Fact::BorrowEnd { loan, to_line, .. } => (loan, to_line, false),
          Fact::HoldEnd { loan, to_line, .. } => (loan, to_line, true),
          _ => continue,
        };
        if id != loan || e.drop_route != crate::facts::DropRoute::Normal {
          continue;
        }
        let caller = matches!(to, ResourceTy::Caller) && e.caller_slot.as_ref().is_some_and(&belongs);
        if !caller && !e.end_places.iter().any(&belongs) {
          continue;
        }
        if hold && (e.continuing_places.is_empty() || matches!(to, ResourceTy::Caller)) {
          continue;
        }
        let (mut s, entry) = self.normal_fork_site(facts, &switches, row, e, next);
        if let Some(&(branch, a)) = s.path.last() {
          let (_, ExternalEvent::Branch { branches, branch_type, .. }) =
            branch_at(self.preprocessed_events, branch, &mut Vec::new()).unwrap()
          else {
            unreachable!()
          };
          if s.path.iter().any(|(id, _)| {
            matches!(
              branch_at(self.preprocessed_events, *id, &mut Vec::new()),
              Some((_, ExternalEvent::Branch { branch_type: BranchType::Loop(..), .. }))
            )
          }) {
            continue;
          }
          if branches.iter().any(|arm| arm.decl_vars.contains(holder)) {
            continue;
          }
          let bb = &self.bwf.body[e.location.block];
          if !entry
            && e.location.statement_index == bb.statements.len()
            && matches!(bb.terminator().kind, TerminatorKind::SwitchInt { .. })
          {
            // The selected successor belongs to a decision with no leg in this drawing.
            continue;
          }
          if entry {
            if let Some(cases) = branches[a].source.and_then(|arm| self.fork_end_cases(arm, loan)) {
              qualified.insert((branch, a), cases);
            }
            let single_entry = branches[a].source.is_some_and(|arm| {
              facts.switches.iter().filter(|s| s.branch == Some(arm.branch) && s.arm == Some(arm.index)).count() == 1
            });
            if single_entry
              && e
                .fieldless_operand
                .as_ref()
                .is_some_and(|p| p.projection.is_empty() && belongs(p) && e.end_places.contains(p))
            {
              fieldless_edges.push((branch, a, e));
              if let (Some(arm), Some(lender)) = (branches[a].source, to.extract_rap()) {
                if let Some(state) = facts
                  .arm_boundaries
                  .get(&arm.branch)
                  .and_then(|arms| arms.get(&arm.index))
                  .and_then(|b| b.entry.as_ref())
                {
                  if lender.owner_local().is_some_and(|l| state.locals.get(lender.name()) == Some(&Some(l)))
                    && state.state.get(lender.name()) == Some(&"own")
                    && state.lent.get(lender.name()).is_none_or(Vec::is_empty)
                  {
                    vacant_lenders.insert((branch, a, *lender.hash()));
                  }
                }
              }
              continue;
            }
            s.row = if !e.continuing_places.is_empty() { row } else { branch_type.get_start_end(a).0 };
            s.before = branches[a].e_data.iter().find(|(l, _)| *l >= s.row).map(|(_, e)| e.get_id());
          }
        }
        // Moving the payload into a binding ends its tenure in the aggregate.
        // The move already represents that handoff on this leg.
        if hold
          && !holder.is_ref()
          && self.bwf.body[e.location.block].statements.get(e.location.statement_index).is_some_and(|st| {
            matches!(&st.kind, StatementKind::Assign(pair) if matches!(&pair.1, Rvalue::Use(Operand::Move(p))
            if holder.owner_local() == Some(p.local)))
          })
        {
          continue;
        }
        if sites.insert((s, hold), e).is_some_and(|other| other.occurrence != e.occurrence) {
          continue 'slots;
        }
      }
      let represented = if !fieldless_edges.is_empty() || !qualified.is_empty() {
        self.fork_return_slots(facts, &switches, &slots, loan, to)
      } else { BTreeMap::new() };
      let events = || facts.rows.iter().flat_map(|row| &row.facts);
      // A fieldless variant cannot hand back a payload. Retire its outer
      // template only after accounting for every other normal end of this
      // loan in a sibling's named reference. An outer bypass may have no
      // end place of its own, even when the nested pattern is understood.
      let sibling_calls = match fieldless_edges.as_slice() {
        &[(branch, arm, edge)] if !vacant_lenders.is_empty() || edge.caller_slot.is_some() => {
          let other_lender = events().any(|other| {
            matches!(other.fact, Fact::BorrowEnd { loan: n, .. } if n != loan)
              && other.occurrence == edge.occurrence
              && other.end_places.iter().any(&belongs)
          });
          if other_lender { None } else {
            self.fieldless_sibling_returns(facts, &switches, &slots, &represented, loan, to, (branch, arm), edge)
          }
        }
        _ => None,
      };
      // Two sibling call returns can share a source row with another fork.
      // Their MIR successors and the drawn source arms identify both sites.
      let all_read =
        sites.len() == 2 && sites.iter().all(|((s, hold), e)| {
          !hold
            && s.path.len() == 1
            && holder.ref_local().is_some()
            && e.exact_end_places.iter().any(&belongs)
            && call_return(&self.bwf.body, e)
        }) && {
          let mut arms = sites.keys().map(|(site, _)| site.path[0]);
          let (branch, a) = arms.next().unwrap();
          let (other, b) = arms.next().unwrap();
          branch == other && a != b
        }
          && events()
            .filter(|e| matches!(e.fact, Fact::BorrowEnd { loan: l, .. } if l == loan))
            .all(|e| sites.values().any(|end| end.occurrence == e.occurrence));
      let cases_complete = !qualified.is_empty() && events()
        .filter(|e| e.drop_route == crate::facts::DropRoute::Normal && matches!(e.fact, Fact::BorrowEnd { loan: l, .. } if l == loan))
        .all(|e| sites.iter().any(|((_, hold), end)| !hold && end.occurrence == e.occurrence)
          || represented.values().any(|end| end.occurrence == e.occurrence));
      if !cases_complete && sibling_calls.is_none() && !all_read {
        continue;
      }
      if let Some(sibling_calls) = sibling_calls {
        for (id, dests) in sibling_calls {
          let (_, slot, _) = slots.iter().find(|(_, slot, _)| slot.get_id() == id).unwrap();
          removed.insert(id);
          for (i, (site, edge)) in dests.into_iter().enumerate() {
            let mut event = slot.clone();
            identify_end(&mut event, i != 0, vec![loan], self.unique_id, &mut self.end_loans);
            self.row_layout.record_arm_point(event.get_id(), edge);
            if let Some(info) = facts.loans.iter().find(|l| l.loan == loan) {
              if let Some((lender, kind)) = self.fork_lender(info).zip(super::cases::borrow_kind(info.kind)) {
                let holder = styled_name_html(&endpoints(slot).0.name());
                let exact = matches!(edge.end_places.as_slice(), [p] if edge.exact_end_places.contains(p));
                self.row_layout.fork_arrow_titles.insert(event.get_id(),
                  super::cases::call_title(&holder, &lender, kind, exact));
              }
            }
            put.push((site, event, false));
          }
        }
        for (_, _, e) in fieldless_edges {
          let successor = e.occurrence.successor.unwrap().block;
          empty_payloads.insert((loan, crate::facts::SwitchEdge { switch: e.location, successor }));
        }
        fieldless_lenders.extend(vacant_lenders.into_iter().map(|key| (key, *holder.hash())));
      }
      let original = template.get_id();
      removed.insert(original);
      let mut dests: Vec<_> = sites.into_iter().collect();
      // Retain the original reservation when this arrow still occupies its row.
      dests.sort_by_key(|((s, _), _)| s.row != *line);
      let mut first = true;
      for ((s, only_holder), edge) in dests {
        if slots.iter().any(|(_, e, p)| {
          !p.is_empty()
            && p == &s.path
            && endpoints(e) == endpoints(template)
            && represented.get(&e.get_id()).is_some_and(|other| other.occurrence == edge.occurrence)
        }) {
          continue;
        }
        if !placed.insert((template.end_slot(), s.clone(), only_holder, loan)) {
          continue;
        }
        // A copied aggregate payload is a handoff. Its lender dot teaches
        // that the binding still carries the same loan after the copy.
        let copy = only_holder
          && !holder.is_ref()
          && self.bwf.body[edge.location.block].statements.get(edge.location.statement_index).is_some_and(|st| {
            matches!(&st.kind, StatementKind::Assign(pair) if matches!(pair.1, Rvalue::Use(Operand::Copy(src))
              if holder.owner_local() == Some(src.local) && edge.continuing_places.iter().any(|p|
                p.projection.is_empty() && pair.0.as_local() == Some(Local::from_usize(p.local)))))
          });
        let mut e = if only_holder && !copy {
          ExternalEvent::RefDie {
            slot: template.end_slot().unwrap(),
            from: ResourceTy::Value(holder.clone()),
            to: to.clone(),
            id: original,
            num_curr_borrowers: 1,
          }
        } else {
          template.clone()
        };
        if copy {
          if let ExternalEvent::StaticDie { still_borrowed_by, .. }
          | ExternalEvent::MutableDie { still_borrowed_by, .. } = &mut e
          {
            *still_borrowed_by = edge
              .continuing_places
              .iter()
              .filter_map(|p| self.raps.place(Local::from_usize(p.local), &p.name).map(|r| r.rap.name().clone()))
              .collect();
          }
        }
        identify_end(&mut e, !first, vec![loan], self.unique_id, &mut self.end_loans);
        // A direct MIR assignment transfers the same reference into its
        // destination. The holder-end dot describes that transfer.
        if only_holder && holder.is_mutref() {
          let moved_to = self.bwf.body[edge.location.block].statements.get(edge.location.statement_index).and_then(|st| {
            let StatementKind::Assign(pair) = &st.kind else { return None };
            let Rvalue::Use(Operand::Move(src)) = pair.1 else { return None };
            let dest = pair.0.as_local()?;
            if src.as_local() != holder.ref_local() { return None; }
            edge.continuing_places.iter().find(|p| p.projection.is_empty() && p.local == dest.index())
          });
          if let Some(dest) = moved_to {
            self.row_layout.holder_handoff_titles.insert(e.get_id(), format!("The mutable reference moves into {} and the loan continues there", styled_name_html(&dest.name)));
          }
        }
        if all_read || qualified.values().any(|cases| cases.calls.contains(&edge.location)) {
          if let Some(info) = facts.loans.iter().find(|info| info.loan == loan) {
            if let Some(kind) = super::cases::borrow_kind(info.kind) {
              let h = styled_name_html(holder.name());
              let title = self.fork_lender(info).map_or_else(
                || format!("{h}'s {kind} borrow ends"),
                |l| super::cases::call_title(&h, &l, kind, edge.exact_end_places.iter().any(&belongs)),
              );
              self.row_layout.fork_arrow_titles.insert(e.get_id(), title);
            }
          }
        }
        if copy {
          let borrow = facts.loans.iter().find(|l| l.loan == loan)
            .and_then(|l| self.fork_lender(l).zip(super::cases::borrow_kind(l.kind)))
            .map_or_else(|| "borrow".to_owned(), |(lender, kind)| format!("{kind} borrow of {lender}"));
          let names = edge.continuing_places.iter().map(|p| styled_name_html(&p.name)).collect::<Vec<_>>().join(", ");
          let h = styled_name_html(holder.name());
          self
            .row_layout
            .fork_arrow_titles
            .insert(e.get_id(), format!("After the reference is copied out of {h}, the {borrow} continues in {names}"));
        }
        if let Some(cases) = s.path.last().and_then(|key| qualified.get(key)) {
          if let ExternalEvent::StaticDie { from, .. } | ExternalEvent::MutableDie { from, .. } = &mut e {
            calls.extend(&cases.calls);
            // All histories leave this holder without its loan. Keep its
            // end dot and state even though the end may precede this leg.
            if cases.common_holder.is_some() && cases.common_holder == holder.ref_local() {
              let mut end = ExternalEvent::RefDie { slot: template.end_slot().unwrap(),
                from: from.clone(), to: ResourceTy::Anonymous, id: original, num_curr_borrowers: 0 };
              identify_end(&mut end, true, vec![loan], self.unique_id, &mut self.end_loans);
              self.row_layout.fork_end_cases.insert(end.get_id(), cases.text.clone());
              self.row_layout.record_arm_point(end.get_id(), edge);
              put.push((s.clone(), end, false));
            }
            *from = ResourceTy::Anonymous;
            self.row_layout.fork_end_cases.insert(e.get_id(), cases.text.clone());
          }
        }
        self.row_layout.record_arm_point(e.get_id(), edge);
        let reserve = first && s.row == *line && !s.path.is_empty() && e.is_var_arrow_ev();
        let stack = reserve && self.event_line_map.get(line).is_some_and(|es| es.len() > 1);
        if reserve {
          reserved.insert(original);
        }
        first = false;
        put.push((s, e, stack));
      }
    }
    self.name_fork_call_ends(&calls);
    self.row_layout.fieldless_lenders.extend(fieldless_lenders);
    remove_except_reserved(self.preprocessed_events, self.event_line_map, &removed, &reserved);
    for (s, e, stack) in put {
      insert(self.preprocessed_events, self.event_line_map, &s.path, s.before, (s.row, e), stack);
    }
    empty_payloads
  }
}
