//! Map loan boundaries from the borrow-checking body into its drawn branches.
//! The source of an ending edge can precede the switch even when it is the
//! assignment in a successor that prompted the walk to draw a give-back.
use crate::expr_visitor::ExprVisitor;
use crate::facts::{Ev, Fact, LoanInfo};
use crate::svg_generator::data::{ExternalEvent, ResourceTy, StaticLender};
use rustc_middle::mir::{Body, Location};
use rustc_span::Span;
use rustc_utils::BodyExt;
use std::collections::{BTreeMap, BTreeSet};

mod cases;
mod exit;
mod normal;

type Events = Vec<(usize, ExternalEvent)>;
type LineMap = BTreeMap<usize, Vec<ExternalEvent>>;
type Path = Vec<(usize, usize)>; // Branch event id, arm index.

#[derive(Default)]
struct RepresentedEnds {
  edges: BTreeSet<(usize, Location, Option<Location>)>,
  arm_loans: BTreeSet<(usize, usize, usize)>, // Branch id, arm, loan.
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// The compiler loan issued by this expression. Equal-looking borrows
  /// in a single statement still have different MIR sites.
  pub(crate) fn loan_at_span(&self, span: Span) -> Option<&'a LoanInfo> {
    let facts = self.body_facts?;
    let mut loans = facts.rows.iter().flat_map(|row| &row.facts).filter_map(|ev| {
      let Fact::Borrow { loan, .. } = ev.fact else { return None };
      let src = self.bwf.body.source_info(ev.location).span;
      (span.with_parent(None) == src.with_parent(None)).then_some(loan)
    });
    let id = loans.next()?;
    if loans.any(|other| other != id) {
      return None;
    }
    facts.loans.iter().find(|l| l.loan == id)
  }

  /// A binding declared inside a drawn arm owns its end on that arm even
  /// when its lender has no leg there. Match the holder on the real MIR
  /// boundary before the renderer expands one-line arms into drawing rows.
  pub(crate) fn place_arm_local_ends(&mut self) {
    let Some(facts) = self.body_facts else { return };
    let mut destinations = BTreeMap::new();
    for (line, e) in self.preprocessed_events.iter() {
      let r = match e {
        ExternalEvent::StaticDie { from, .. } | ExternalEvent::MutableDie { from, .. } | ExternalEvent::RefDie { from, .. } => {
          from.extract_rap()
        }
        ExternalEvent::OrAltBorrow { into, end: true, .. } => Some(into),
        _ => None,
      };
      let Some(r) = r else { continue };
      let Some(col) = facts.columns.iter().find(|c| &c.name == r.name()) else { continue };
      let body = &self.bwf.body;
      let path_of = |mut arms: Vec<crate::facts::SourceArm>, scope| {
        arms.extend(crate::facts::enclosing_arms(self.tcx, scope, true));
        let mut path = Vec::new();
        declared_at(self.preprocessed_events, r, &arms, &mut path).then_some(path)
      };
      let locals = facts.bindings.columns.get(&col.root);
      let decls: BTreeSet<_> = locals.into_iter().flatten().filter_map(|&local| {
        if r.ref_local().is_some_and(|l| l != local) { return None; }
        let pat = body.local_decls[local].source_info.scope.lint_root(&body.source_scopes)?;
        path_of(Vec::new(), pat)
      }).collect();
      let Some(row) = facts.rows.iter().find(|row| row.line == *line) else { continue };
      let mut paths = BTreeSet::new();
      let emitted = self.end_loans.get(&e.get_id());
      for ev in &row.facts {
        let related = match &ev.fact {
          Fact::BorrowEnd { loan, holder, .. } | Fact::HoldEnd { loan, holder, .. } => {
            emitted.is_some_and(|ls| ls.contains(loan)) && holder.split('|').any(|h| h == col.name || h == col.root)
          }
          Fact::ScopeEnd { path, .. } => path == &col.name || path == &col.root,
          _ => false,
        };
        if !related || r.ref_local().zip(ev.place_local).is_some_and(|(r, p)| p != r) {
          continue;
        }
        let arms = crate::facts::location_arms(self.tcx, body, facts, ev.display_location);
        if let Some(path) = path_of(arms, body.location_to_hir_id(ev.display_location)) {
          paths.insert(path);
        }
      }
      // A call may use the loan on an unwind-only exit, or the slot may
      // hand it on rather than giving it back. A MIR use of this local at
      // the selected boundary still identifies its arm.
      if paths.is_empty() {
        for ev in &row.facts {
          let use_here = match &ev.fact {
            Fact::Copy { path, .. } | Fact::Move { path, .. } | Fact::Init { path, .. } => path == &col.name || path == &col.root,
            _ => false,
          };
          if use_here && !r.ref_local().zip(ev.place_local).is_some_and(|(r, p)| p != r) {
            if let Some(path) = path_of(crate::facts::location_arms(self.tcx, body, facts, ev.location), body.location_to_hir_id(ev.location)) {
              paths.insert(path);
            }
          }
        }
      }
      // A compiler source scope for nested bookkeeping may only
      // identify an ancestor. The local's MIR declaration disambiguates
      // that ancestor's arm when all the edge evidence agrees with it.
      if decls.len() == 1 && paths.iter().all(|p| decls.first().unwrap().starts_with(p)) {
        let path = decls.first().unwrap();
        // The MIR declaration selects the complete path. A synthetic
        // scope-end row outside the arm cannot be drawn on its leg.
        let mut list = &*self.preprocessed_events;
        let mut inside = true;
        for &(id, a) in path {
          let Some((_, ExternalEvent::Branch { branches, .. })) = list.iter().find(|(_, e)| e.get_id() == id) else { break };
          let branch = &branches[a];
          inside &= branch.source_span.is_some_and(|s| s.start.0 <= *line && *line <= s.end.0);
          list = &branch.e_data;
        }
        if inside {
          paths.clear();
          paths.insert(path.clone());
        }
      }
      if paths.len() == 1 {
        destinations.insert(e.get_id(), paths.pop_first().unwrap());
      }
    }
    let mut moved = Vec::new();
    self.preprocessed_events.retain(|(line, e)| {
      let Some(path) = destinations.get(&e.get_id()) else { return true };
      moved.push((path, (*line, e.clone())));
      false
    });
    for (path, ev) in moved {
      let mut list = &*self.preprocessed_events;
      let mut along = false;
      let mut one_line = false;
      let lender = match &ev.1 {
        ExternalEvent::StaticDie { to, .. } | ExternalEvent::MutableDie { to, .. } => to.extract_rap(),
        ExternalEvent::OrAltBorrow { ro, .. } => Some(ro),
        _ => None,
      };
      for &(id, a) in path {
        let Some((_, ExternalEvent::Branch { branches, live_vars, split_point, merge_point, .. })) =
          list.iter().find(|(_, e)| e.get_id() == id)
        else {
          break;
        };
        one_line |= split_point == merge_point && *split_point == ev.0;
        along |= lender.is_some_and(|r| live_vars.contains(r) || branches[a].decl_vars.contains(r));
        list = &branches[a].e_data;
      }
      let stacked = self.event_line_map.get(&ev.0).is_some_and(|g| g.len() > 1);
      // An arm local's end can share a row with arrows emitted outside
      // that arm. Keep the stack on the parent row unless the end belongs
      // to the arm's own expanded one-line row.
      let in_arm_stack = one_line && !along && lender.is_some();
      if in_arm_stack {
        if let Some(group) = self.event_line_map.get_mut(&ev.0) {
          group.retain(|e| e.get_id() != ev.1.get_id());
        }
      }
      let fork = lender.filter(|_| along).cloned();
      insert_local(self.preprocessed_events, path, ev, in_arm_stack || stacked, fork.as_ref());
    }
  }

  pub(crate) fn place_repointed_ends(&mut self) {
    let Some(facts) = self.body_facts else { return };
    if self.end_loans.is_empty() {
      return;
    }
    let mut templates = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), Some(&self.end_loans), &mut templates);
    if templates.is_empty() {
      return;
    }
    let switches = self.branch_switches();
    // A walk-time overwrite and a synthesized end can represent the same
    // loan in the same slot. Keep the original teaching event for that
    // boundary. Other loans reaching the same columns remain distinct.
    let walk: Vec<_> = templates
      .iter()
      .filter(|(_, e, _)| e.get_id() < self.first_synthesized_end)
      .map(|(row, e, path)| (*row, e.clone(), path.clone()))
      .collect();
    let mut emitted = BTreeSet::new();
    let mut remove = BTreeSet::new();
    let mut put = Vec::new();
    for (line, ev, origin) in templates {
      let ids = self.end_loans[&ev.get_id()].clone();
      if ev.get_id() >= self.first_synthesized_end {
        if walk.iter().any(|(row, other, path)| {
          *row == line
            && *path == origin
            && std::mem::discriminant(other) == std::mem::discriminant(&ev)
            && endpoints(other) == endpoints(&ev)
            && self.end_loans.get(&other.get_id()) == Some(&ids)
        }) {
          remove.insert(ev.get_id());
        }
        continue;
      }
      // A literal's self-arrow describes replacement of the slot; no
      // resource is released by the aggregate's later region boundary.
      if matches!(&ev, ExternalEvent::StaticDie { static_lender: Some(StaticLender::Literal { .. }), .. }) {
        continue;
      }
      let (from, to) = endpoints(&ev);
      let holder = from.real_name();
      let own = matches!(ev, ExternalEvent::RefDie { .. });
      // These overwrite templates get a loan ID only when each edge is
      // the assignment's drop or an explicit drop of the old holder.
      let dropped = self.field_overwrite_ends.remove(&ev.get_id());
      let mut sites = BTreeMap::<Site, BTreeSet<usize>>::new();
      let mut lender_only = BTreeSet::new();
      let mut replacement_drops = BTreeMap::new();
      let mut drop_holder = None;
      for row in &facts.rows {
        for edge in &row.facts {
          let (loan, h, next, is_own) = match &edge.fact {
            Fact::BorrowEnd { loan, holder, to_line, .. } => (loan, holder, to_line, false),
            Fact::HoldEnd { loan, holder, to_line, .. } => (loan, holder, to_line, true),
            _ => continue,
          };
          if !ids.contains(loan) || dropped && is_own {
            continue;
          }
          if dropped {
            let mut site = Site { path: Vec::new(), before: None, row: row.line };
            self.locate(self.preprocessed_events, &switches, edge.display_location, edge, *next, &mut site);
            // The field is gone at either drop. Mark the release on
            // the lender without ending the new field's borrow.
            if let Some((name, conditional)) = row.facts.iter().find_map(|ev| match &ev.fact {
              Fact::Drop { path, at_reassign: true, how, .. } if ev.location == edge.location =>
                Some((path.as_str(), *how == "drops_if_init")),
              _ => None,
            }) {
              drop_holder = Some(name);
              replacement_drops.insert(site.clone(), conditional);
            }
            lender_only.insert(site.clone());
            sites.entry(site).or_default().insert(*loan);
            continue;
          }
          let Some(info) = facts.loans.iter().find(|l| l.loan == *loan) else { continue };
          if info.source == "static" { continue; }
          let mine = h.split('|').any(|h| h == holder || holder.strip_prefix(h).is_some_and(|part| part.starts_with('.')));
          let drawn = to.real_name();
          let lender = info.lender == drawn
            || info.through.as_ref() == Some(&drawn)
            || (to.is_ref() && info.direct_holders.iter().any(|n| n == &drawn));
          if !mine || !lender {
            continue;
          }
          let still_lent = matches!(&ev, ExternalEvent::StaticDie { still_borrowed_by, .. }
            | ExternalEvent::MutableDie { still_borrowed_by, .. } if !still_borrowed_by.is_empty());
          if is_own && !(own || still_lent) {
            continue;
          }
          if !is_own
            && (own || still_lent)
            && facts
              .rows
              .iter()
              .flat_map(|r| &r.facts)
              .any(|f| matches!(&f.fact, Fact::HoldEnd { loan: l, holder: h, .. } if l == loan && h == &holder))
          {
            continue;
          }
          let mut site = Site { path: Vec::new(), before: None, row: row.line };
          self.locate(self.preprocessed_events, &switches, edge.display_location, edge, *next, &mut site);
          // Region inference can keep b restricted after x = &b; x = y.
          // If MIR ended x's holding and overwrote that value on every path
          // here, the eventual loan end belongs on b alone (#1742).
          if !own && !is_own && ids.as_slice() == [*loan] && h == &holder {
            let before = |a, b| a != b && dominates(&self.bwf.body, a, b);
            let stores = facts.rows.iter().flat_map(|r| &r.facts).filter(|f|
              matches!(&f.fact, Fact::Repoint { holder: slot, reaching: Some(old), new, .. }
                if slot == h && old.contains(loan) && !new.contains(loan)) && before(f.location, edge.location));
            if stores.into_iter().any(|store| {
              let mut events = facts.rows.iter().flat_map(|r| &r.facts);
              let released = events.clone().any(|f|
                matches!(&f.fact, Fact::HoldEnd { holder: slot, loan: id, .. } if slot == h && id == loan)
                  && dominates(&self.bwf.body, f.location, store.location));
              let can_restore = events.any(|f|
                matches!(&f.fact, Fact::Repoint { holder: slot, new, .. } if slot == h && new.contains(loan))
                  && !before(f.location, store.location) && !before(edge.location, f.location));
              released && !can_restore
            }) {
              lender_only.insert(site.clone());
            }
          }
          sites.entry(site).or_default().insert(*loan);
        }
      }
      if sites.is_empty() {
        continue;
      }
      let site = if sites.len() == 1 {
        sites.first_key_value().unwrap().0.clone()
      } else {
        // P2-1b may already have a separate end on another path (the
        // outer holder after the join, for example). The presentation at
        // this row belongs to the common parent until paths are expanded.
        let paths: Vec<_> = sites.keys().map(|s| &s.path).collect();
        let mut common = paths[0].clone();
        common.truncate((0..common.len()).take_while(|&i| paths.iter().all(|p| p.get(i) == common.get(i))).count());
        if sites.keys().any(|s| s.row == line && s.path == origin) {
          Site { path: origin.clone(), before: None, row: line }
        } else if let Some(boundary) = sites.keys().find(|s| s.row == line && s.before.is_some()) {
          boundary.clone()
        } else {
          Site { path: common, before: None, row: line }
        }
      };
      let individual = dropped || site.before.is_some() && sites.len() > 1;
      let destinations = if individual { sites } else { BTreeMap::from([(site, ids.iter().copied().collect())]) };
      let mut first = true;
      for (site, loans) in destinations {
        let ids: Vec<_> = loans.into_iter().collect();
        let key = (
          ids.clone(),
          from.extract_rap().map(|r| *r.hash()),
          to.extract_rap().map(|r| *r.hash()),
          own,
          site.clone(),
        );
        if !emitted.insert(key) {
          remove.insert(ev.get_id());
          continue;
        }
        let detached = lender_only.contains(&site);
        if !detached && !individual && site.matches(self.preprocessed_events, &origin, line, ev.get_id()) {
          continue;
        }
        remove.insert(ev.get_id());
        let mut e = ev.clone();
        identify_end(&mut e, !first, ids, self.unique_id, &mut self.end_loans);
        if detached {
          if let ExternalEvent::StaticDie { from, .. } | ExternalEvent::MutableDie { from, .. } = &mut e {
            *from = ResourceTy::Anonymous;
          }
        }
        if let Some(parent) = drop_holder {
          self.row_layout.dropped_field_loans.insert(e.get_id(), crate::event_order::DroppedFieldLoan {
            field: holder.clone(), holder: parent.to_owned(), replacement: replacement_drops.get(&site).copied(),
          });
        }
        first = false;
        put.push((site, e));
      }
    }
    remove_from(self.preprocessed_events, self.event_line_map, &remove);
    for (site, e) in put {
      insert(self.preprocessed_events, self.event_line_map, &site.path, site.before, (site.row, e), true);
    }
  }

  /// Mutually exclusive last holders. When another arm already gives this
  /// exact loan back through its local binding, the outer slot gives it
  /// back only on its own ending edges. Its copy ends where MIR stops
  /// counting that holder.
  pub(crate) fn place_last_holders(&mut self) {
    let Some(facts) = self.body_facts else { return };
    let mut slots = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), Some(&self.end_loans), &mut slots);
    let switches = self.branch_switches();
    let mut remove = BTreeSet::new();
    let mut put = Vec::new();
    for (line, ev, path) in &slots {
      let (from, to) = endpoints(ev);
      if !path.is_empty() || !matches!(ev, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) {
        continue;
      }
      let Some(r) = from.extract_rap() else { continue };
      let loans = &self.end_loans[&ev.get_id()];
      let &[loan_id] = loans.as_slice() else { continue };
      let loan = &loan_id;
      let ends: Vec<_> = facts
        .rows
        .iter()
        .flat_map(|row| {
          row.facts.iter().filter_map(move |e| match &e.fact {
            Fact::BorrowEnd { loan: id, holder, to_line, .. } if id == loan => Some((row.line, e, holder, *to_line)),
            _ => None,
          })
        })
        .collect();
      if !ends.iter().all(|(l, _, _, _)| l == line) || !ends.iter().any(|(_, _, h, _)| *h == r.name()) {
        continue;
      }
      let mut mine = BTreeSet::new();
      let mut others = BTreeSet::new();
      for (row, e, h, next) in ends {
        let mut site = Site { path: Vec::new(), before: None, row };
        self.locate(self.preprocessed_events, &switches, e.location, e, next, &mut site);
        if h == r.name() {
          mine.insert(site);
        } else if slots.iter().any(|(_, ae, arm)| {
          !arm.is_empty()
            && *arm == site.path
            && endpoints(ae).0.real_name() == *h
            && endpoints(ae).1 == to
            && self.end_loans.get(&ae.get_id()).is_some_and(|ids| ids.contains(loan))
        }) {
          others.insert(site.path);
        } else {
          others.clear();
          break;
        }
      }
      if others.is_empty() || mine.is_empty() || mine.iter().any(|s| s.path.is_empty() || others.contains(&s.path)) {
        continue;
      }
      // A later exit pass handles a conditional that reaches its join.
      // If all paths leave here the last holder on each exit is final.
      if !mine.iter().all(|s| {
        s.path.last().is_some_and(|(id, _)| {
          matches!(branch_at(self.preprocessed_events,*id,&mut Vec::new()),
          Some((_, ExternalEvent::Branch { branches, .. })) if branches.iter().all(|b|b.diverges))
        })
      }) {
        continue;
      }
      remove.insert(ev.get_id());
      for site in mine {
        let mut copy = ev.clone();
        if let ExternalEvent::StaticDie { id, .. } | ExternalEvent::MutableDie { id, .. } = &mut copy {
          *id = *self.unique_id;
          *self.unique_id += 1;
          self.end_loans.insert(*id, vec![*loan]);
        }
        put.push((site, copy));
      }
      for (row, e) in &facts.inline_holder_ends {
        let Fact::HoldEnd { loan: id, holder, to_line, .. } = &e.fact else { continue };
        if id != loan || holder != r.name() {
          continue;
        }
        let mut site = Site { path: Vec::new(), before: None, row: *row };
        self.locate(self.preprocessed_events, &switches, e.location, e, *to_line, &mut site);
        if !others.contains(&site.path) {
          continue;
        }
        let id = *self.unique_id;
        *self.unique_id += 1;
        self.end_loans.insert(id, vec![*loan]);
        put.push((site, ExternalEvent::RefDie { slot: id, id, from: from.clone(), to: to.clone(), num_curr_borrowers: 1 }));
      }
    }
    remove_from(self.preprocessed_events, self.event_line_map, &remove);
    for (site, e) in put {
      insert(self.preprocessed_events, self.event_line_map, &site.path, site.before, (site.row, e), false);
    }
  }

  /// An end that is already inside an arm can have several ending
  /// edges in its nested conditional. Restrict the search to that arm.
  pub(crate) fn place_nested_ends(&mut self) {
    let Some(facts) = self.body_facts else { return };
    let switches = self.branch_switches();
    let mut slots = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), None, &mut slots);
    let mut remove = BTreeSet::new();
    let mut put = Vec::new();
    for (line, ev, origin) in slots {
      // The normal-fork pass has already placed this return on its
      // particular MIR edge, including a call after a nested decision.
      if self.row_layout.has_arm_point(ev.get_id()) { continue; }
      let (from, to) = endpoints(&ev);
      let (Some(r), lender) = (from.extract_rap(), to.extract_rap()) else { continue };
      if lender.is_some_and(|r| r.is_fn()) {
        continue;
      }
      let Some((&(id, a), _)) = origin.split_last() else { continue };
      let Some((_, ExternalEvent::Branch { branches, .. })) = branch_at(self.preprocessed_events, id, &mut Vec::new()) else {
        continue;
      };
      let arm = &branches[a];
      if !arm.e_data.iter().any(|(_, e)| matches!(e, ExternalEvent::Branch { .. })) {
        continue;
      }
      let holder = r.name();
      let root = facts.columns.iter().find(|c| &c.name == holder).map(|c| &c.root);
      let mut loans = self.end_loans.get(&ev.get_id()).cloned().unwrap_or_default();
      let is_own = matches!(ev, ExternalEvent::RefDie { .. });
      if loans.is_empty() {
        // Match and closure callbacks emit their own slots. Require a
        // single local declared in this arm and a single MIR loan of
        // the drawn lender that reached this binding.
        let mut locals = facts.bindings.columns.get(holder).into_iter().flatten().filter(|&&local| {
          let scope = self.bwf.body.local_decls[local].source_info.scope;
          scope.lint_root(&self.bwf.body.source_scopes).is_some_and(|pat|
            arm.source.is_some_and(|source| crate::facts::enclosing_arms(self.tcx, pat, true).contains(&source)))
        });
        let Some(&local) = locals.next() else { continue };
        if locals.next().is_some() { continue; }
        let inits: Vec<_> = facts
          .rows
          .iter()
          .flat_map(|row| &row.facts)
          .filter_map(|f| {
            let Fact::Init { path, .. } = &f.fact else { return None };
            let block = &self.bwf.body.basic_blocks[f.location.block];
            let stmt = block.statements.get(f.location.statement_index)?;
            let rustc_middle::mir::StatementKind::Assign(pair) = &stmt.kind else { return None };
            (path == holder && pair.0.local == local).then_some(f.location)
          })
          .collect();
        let matched: Vec<_> = facts
          .loans
          .iter()
          .filter(|l| {
            l.holders.contains(holder)
              && lender.is_some_and(|r| &l.lender == r.name())
              && facts.rows.iter().flat_map(|row| &row.facts).any(|issue| {
                matches!(issue.fact,Fact::Borrow{loan,..} if loan==l.loan)
                  && inits.iter().any(|&at| dominates(&self.bwf.body, issue.location, at))
              })
          })
          .map(|l| l.loan)
          .collect();
        if matched.len() != 1 {
          continue;
        }
        loans = matched;
      }
      let mut candidates = Vec::new();
      for (row, e) in facts
        .rows
        .iter()
        .flat_map(|r| r.facts.iter().map(move |e| (r.line, e)))
        .chain(facts.inline_holder_ends.iter().map(|(row, e)| (*row, e)))
      {
        let (loan, h, next, own) = match &e.fact {
          Fact::BorrowEnd { loan, holder, to_line, .. } => (loan, holder, *to_line, false),
          Fact::HoldEnd { loan, holder, to_line, .. } => (loan, holder, *to_line, true),
          _ => continue,
        };
        if !loans.contains(loan) {
          continue;
        }
        let mut site = Site { path: Vec::new(), row, before: None };
        self.locate(&arm.e_data, &switches, e.display_location, e, next, &mut site);
        // The branch into an arm may first pass an imaginary edge or a
        // discriminant test. Its first executable successor has the arm span.
        if site.path.is_empty() {
          if let Some(next_loc) = e.occurrence.successor {
            let mut after = Site { path: Vec::new(), row: next, before: None };
            self.locate(&arm.e_data, &switches, next_loc, e, next, &mut after);
            if !after.path.is_empty() {
              site = after;
            }
          }
        }
        let Some(&(nested, _)) = site.path.first() else { continue };
        let Some((_, ExternalEvent::Branch { branches: arms, live_vars, branch_type, .. })) =
          arm.e_data.iter().find(|(_, e)| e.get_id() == nested)
        else {
          continue;
        };
        if matches!(branch_type, crate::svg_generator::data::BranchType::Loop(..))
          || arms.iter().any(|b| b.decl_vars.contains(r))
          || (!live_vars.contains(r) && !lender.is_some_and(|l| live_vars.contains(l)))
        {
          continue;
        }
        // The edge into a branch releases this loan before its first
        // instruction, including a new shared loan of that same place.
        if let Some(&(id, a)) = site.path.last() {
          if let Some((_, ExternalEvent::Branch { branches, .. })) = branch_at(&arm.e_data, id, &mut Vec::new()) {
            let entered = &branches[a];
            if next == site.row && !entered.source.is_some_and(|s| crate::facts::location_arms(self.tcx, &self.bwf.body, facts, e.location).contains(&s)) {
              site.before = entered.e_data.iter().find(|(l, _)| *l >= site.row).map(|(_, e)| e.get_id());
            }
          }
        }
        candidates.push((h.as_str(), site, own && !is_own, *loan));
      }
      let dests = nested_destinations(candidates, holder, root.map(String::as_str), line);
      if dests.is_empty() {
        continue;
      }
      let listed = arm.line_map.get(&line).is_some_and(|g| g.iter().any(|x| x.get_id() == ev.get_id()));
      remove.insert(ev.get_id());
      for (i, ((s, only_holder), ids)) in dests.into_iter().enumerate() {
        let mut event = if only_holder {
          ExternalEvent::RefDie { slot: ev.end_slot().unwrap_or(ev.get_id()), from: from.clone(), to: to.clone(), id: ev.get_id(), num_curr_borrowers: 1 }
        } else {
          ev.clone()
        };
        identify_end(&mut event, i > 0, ids.into_iter().collect(), self.unique_id, &mut self.end_loans);
        let mut path = origin.clone();
        path.extend(s.path);
        put.push((path, s.before, (s.row, event), listed && line == s.row));
      }
    }
    // Top-level stack entries reserve the row on which the original
    // arrow is drawn in its nested arm.
    remove_from(self.preprocessed_events, &mut BTreeMap::new(), &remove);
    for (p, b, e, listed) in put {
      insert(self.preprocessed_events, self.event_line_map, &p, b, e, listed);
    }
  }

  fn branch_switches(&self) -> BTreeMap<usize, Vec<Location>> {
    fn walk(events: &Events, facts: &crate::facts::BodyFacts, switches: &mut BTreeMap<usize, Vec<Location>>) {
      for (_, event) in events {
        let ExternalEvent::Branch { id, branches, .. } = event else { continue };
        if let Some(source) = branches.first().and_then(|b| b.source) {
          switches.insert(*id, facts.tests_of(source.branch).into_iter().collect());
        }
        for b in branches { walk(&b.e_data, facts, switches); }
      }
    }
    let mut switches = BTreeMap::new();
    if let Some(facts) = self.body_facts { walk(self.preprocessed_events, facts, &mut switches); }
    switches
  }

  /// Stored call arguments have a loan id but no separate reference
  /// column. Their return marks only the lender. At a move, MIR names
  /// the holder that actually carries the loan on each outgoing path.
  pub(crate) fn place_stored_ends(&mut self) {
    let Some(facts) = self.body_facts else { return };
    if self.stored_borrows.is_empty() {
      return;
    }
    let loans: BTreeMap<_, _> = self.stored_borrows.iter().map(|s| (s.id + 1, s.loans.clone())).collect();
    let mut templates = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), Some(&loans), &mut templates);
    let switches = self.branch_switches();
    let mut remove = BTreeSet::new();
    let mut put = Vec::new();
    for (line, ev, origin) in templates {
      let (ResourceTy::Index(current, ..), _) = endpoints(&ev) else { continue };
      let ids = &loans[&ev.get_id()];
      let mut dest = BTreeSet::new();
      for row in &facts.rows {
        for end in &row.facts {
          let Fact::BorrowEnd { loan, holder, to_line, .. } = &end.fact else { continue };
          if !ids.contains(loan) {
            continue;
          }
          let mut site = Site { path: Vec::new(), before: None, row: row.line };
          self.locate(self.preprocessed_events, &switches, end.location, end, *to_line, &mut site);
          if let Some(&(id, a)) = site.path.last() {
            if switches.get(&id).is_some_and(|ss| ss.contains(&end.location)) {
              if let Some((_, ExternalEvent::Branch { branch_type, branches, .. })) = branch_at(self.preprocessed_events, id, &mut Vec::new()) {
                site.row = branch_type.get_start_end(a).0;
                site.before = branches[a].e_data.first().map(|(_, e)| e.get_id());
              }
            }
          }
          dest.insert((site, holder.clone()));
        }
      }
      if dest.iter().all(|(site, holder)| site.matches(self.preprocessed_events, &origin, line, ev.get_id()) && holder == current.name()) {
        continue;
      }
      let stay = dest.len() == 1 && dest.first().is_some_and(|(site, _)| site.matches(self.preprocessed_events, &origin, line, ev.get_id()));
      if !stay { remove.insert(ev.get_id()); }
      for (i, (site, name)) in dest.into_iter().enumerate() {
        let mut e = ev.clone();
        let (id, from) = match &mut e {
          ExternalEvent::StaticDie { id, from, .. } | ExternalEvent::MutableDie { id, from, .. } => (id, from),
          _ => continue,
        };
        if i > 0 {
          *id = *self.unique_id;
          *self.unique_id += 1;
        }
        self.end_loans.insert(*id, ids.clone());
        if let (ResourceTy::Index(r, label, _), Some(to)) = (from, self.raps.get(&name)) {
          if label == r.name() {
            *label = name;
          }
          *r = to.rap.clone();
        }
        if stay {
          replace_end(self.preprocessed_events, self.event_line_map, &e);
        } else {
          put.push((site, e));
        }
      }
    }
    remove_from(self.preprocessed_events, self.event_line_map, &remove);
    for (s, e) in put {
      insert(self.preprocessed_events, self.event_line_map, &s.path, s.before, (s.row, e), true);
    }
  }

  /// The successive reference values of one slot. Stores on alternative
  /// control-flow paths cannot form a sequence. Each borrowing arrow
  /// carries the ID of the reference that its RHS produced.
  pub(crate) fn record_store_order(&self, order: &mut BTreeMap<usize, (usize, usize)>) {
    let Some(facts) = self.body_facts else { return };
    if self.repoint_borrows.len() < 2 { return; }
    let mut slots = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), Some(&self.end_loans), &mut slots);
    for row in &facts.rows {
      let mut writes = BTreeMap::<_, Vec<_>>::new();
      for e in &row.facts {
        if let Fact::Repoint { holder, new, reaching: Some(old), .. } = &e.fact {
          writes.entry(holder).or_default().push((e, new, old));
        }
      }
      for (holder, stores) in writes {
        // Require both execution order and reaching definitions. Two stores
        // in different arms can have the same source row and the same slot.
        if stores.len() < 2 || !stores.windows(2).all(|w|
          dominates(&self.bwf.body, w[0].0.location, w[1].0.location) && w[0].1 == w[1].2)
          || !stores.iter().all(|(_, new, _)| matches!(new.as_slice(), [l] if self.repoint_borrows.values().any(|b| b == l)))
        {
          continue;
        }
        let mut effects = BTreeMap::new();
        for &(store, new, _) in &stores {
          for (&id, &loan) in &self.repoint_borrows {
            if new.contains(&loan) { effects.insert(id, store); }
          }
        }
        // Overwriting a reference need not release its lender. Match the
        // return arrow to the actual loan's end, even for an earlier store.
        for (_, event, _) in slots.iter().filter(|(l, e, _)| *l == row.line && endpoints(e).0.name() == *holder) {
          let id = event.get_id();
          let ids = &self.end_loans[&id];
          let lender = endpoints(event).1.name();
          let mut ends = row.facts.iter().filter(|e| matches!(&e.fact,
            Fact::BorrowEnd { loan, lender: from, .. } if ids.contains(loan) && *from == lender));
          if let Some(end) = ends.next().filter(|first| ends.all(|e| e.location == first.location && e.occurrence.successor == first.occurrence.successor)) {
            effects.insert(id, end);
          }
        }
        let Some((&group, _)) = effects.first_key_value() else { continue };
        let mut effects: Vec<_> = effects.into_iter().collect();
        effects.sort_by_key(|(_, e)| e.order);
        // A loan can end on just one edge out of a block. Its end only
        // precedes a later effect on a path through that successor.
        if !effects.windows(2).all(|w| {
          let (a, b) = (w[0].1, w[1].1);
          if a.order == b.order { return a.occurrence.successor == b.occurrence.successor; }
          dominates(&self.bwf.body, a.location, b.location)
            && a.occurrence.successor.is_none_or(|next| dominates(&self.bwf.body, next, b.location))
        }) {
          continue;
        }
        for (rank, same_point) in effects.chunk_by(|a, b| a.1.order == b.1.order).enumerate() {
          order.extend(same_point.iter().map(|(id, _)| (*id, (group, rank))));
        }
      }
    }
  }

  /// Associate existing return dots with their compiler edges. An event on
  /// another path, or one that could describe several edges, cannot cover
  /// this edge. Pattern bindings also have a loan identity at their MIR init.
  fn represented_edges(&self, switches: &BTreeMap<usize, Vec<Location>>) -> RepresentedEnds {
    let Some(facts) = self.body_facts else { return Default::default() };
    let mut slots = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), None, &mut slots);
    let mut represented = RepresentedEnds::default();
    for (row, ev, path) in slots {
      if !matches!(&ev, ExternalEvent::MutableDie { still_borrowed_by, .. } | ExternalEvent::StaticDie { still_borrowed_by, .. }
        if still_borrowed_by.is_empty()) { continue; }
      let (from, to) = endpoints(&ev);
      let holder = from.real_name();
      let lender = to.real_name();
      let mut bound = BTreeSet::new();
      for &(id, a) in &path {
        if let Some(patterns) = self.arm_bindings.get(&id) {
          let local = from.extract_rap().and_then(|r| r.ref_local().or_else(|| r.owner_local()));
          for &(_, local) in patterns.iter().filter(|&&(arm, p)| arm == a && Some(p) == local) {
            if let Some(inits) = facts.binding_loans.get(&local) {
              bound.extend(inits.values().flatten().copied());
            }
          }
        }
      }
      if let Some(ids) = self.end_loans.get(&ev.get_id()) { bound = ids.iter().copied().collect(); }
      for &loan in &bound {
        if facts.loans.iter().any(|l| l.loan == loan && l.lender == lender) {
          represented.arm_loans.extend(path.iter().map(|&(id, a)| (id, a, loan)));
        }
      }
      let mut candidates = Vec::new();
      for r in &facts.rows {
        for e in &r.facts {
          let Fact::BorrowEnd { loan, holder: h, lender: l, to_line, .. } = &e.fact else { continue };
          if !bound.contains(loan) || *l != lender || !h.split('|').any(|n| n == holder) { continue; }
          let mut site = Site { path: Vec::new(), before: None, row: r.line };
          self.locate(self.preprocessed_events, switches, e.location, e, *to_line, &mut site);
          if site.matches(self.preprocessed_events, &path, row, ev.get_id()) {
            candidates.push((*loan, e.location, e.occurrence.successor));
          }
        }
      }
      if let [edge] = candidates.as_slice() { represented.edges.insert(*edge); }
    }
    represented
  }

  /// Loans taken into pattern bindings are also ended on the outgoing
  /// edge that skips the binding. The holder on that edge may be the
  /// scrutinee itself or a call result without a column.
  pub(crate) fn place_scrutinee_ends(&mut self) {
    if self.arm_bindings.is_empty() { return; }
    let Some(facts) = self.body_facts else { return };
    let switches = self.branch_switches();
    let body = &self.bwf.body;
    let mut put = Vec::new();
    let mut remove = BTreeSet::new();
    let mut represented = self.represented_edges(&switches);
    for (&id, patterns) in &self.arm_bindings {
      if self.let_else_branches.contains(&id) { continue; }
      let mut parent = Vec::new();
      let Some((_, ExternalEvent::Branch { branches, branch_type, .. })) = branch_at(self.preprocessed_events, id, &mut parent)
      else {
        continue;
      };
      for loan in &facts.loans {
        // The declaration identifies a pattern binding even when sibling
        // patterns use the same spelling. Keep its local and its arm.
        let bindings: Vec<_> = patterns.iter().copied().filter(|&(_, local)| {
          facts.binding_loans.get(&local).is_some_and(|inits| inits.values().any(|loans| loans.contains(&loan.loan)))
        }).collect();
        if loan.source != "borrow" || !bindings.iter().any(|(arm, _)| represented.arm_loans.contains(&(id, *arm, loan.loan))) {
          continue;
        }
        let binding = |local| bindings.iter().any(|&(_, p)| p == local);
        // A loan first issued directly to a pattern binding cannot be
        // outstanding on the path that skipped the binding.
        if loan.via_call.is_empty() && loan.direct_locals.iter().all(|&local| binding(local)) { continue; }
        let Some(lender) = loan.lender_local.and_then(|l| self.raps.place(l, &loan.lender)).map(|r| &r.rap) else { continue };
        let Some(issue) = facts.rows.iter().flat_map(|r| &r.facts).find(|e|
          matches!(e.fact, Fact::Borrow { loan: id, .. } if id == loan.loan)) else { continue };
        let ends: Vec<_> = facts
          .rows
          .iter()
          .flat_map(|r| {
            r.facts.iter().filter_map(move |e| match &e.fact {
              Fact::BorrowEnd { loan: id, holder, to_line, .. } if *id == loan.loan => Some((r.line, e, holder, *to_line)),
              _ => None,
            })
          })
          .collect();
        for (row, e, h, next) in &ends {
          let mut site = Site { path: Vec::new(), before: None, row: *row };
          self.locate(self.preprocessed_events, &switches, e.location, e, *next, &mut site);
          let entry = switches.get(&id).is_some_and(|ss| ss.contains(&e.location));
          let index = site.path.iter().find(|(b, _)| *b == id).map(|(_, a)| *a);
          let Some(a) = index else { continue };
          if a >= branches.len() || bindings.iter().any(|(i, _)| *i == a) {
            continue;
          }
          let arm = &branches[a];
          if !entry && !arm.source.is_some_and(|s| crate::facts::location_arms(self.tcx, body, facts, e.location).contains(&s)) {
            continue;
          }
          // A conditional constructor may never have issued this loan
          // on the failed pattern path (Some(&mut b) versus None).
          if !dominates(body, issue.location, e.location) {
            continue;
          }
          // The outer holder can also give this loan back on an edge
          // bypassing the entire nested test. Keep its outer slot for
          // placement across that enclosing switch.
          if ends.iter().any(|(_, other, name, _)| {
            name == h && switches.get(&id).is_some_and(|ss| ss.iter().all(|&sw| !dominates(body, sw, other.location)))
          }) {
            continue;
          }
          // A Copy gives the successful arm two real holders in turn.
          // Its own P2-1b slot records that the binding keeps the loan.
          let copy_into_binding = facts.rows.iter().flat_map(|r| &r.facts).any(|f| {
            if !matches!(f.fact, Fact::Copy { .. }) { return false; }
            let Some(st) = body.basic_blocks[f.location.block].statements.get(f.location.statement_index) else { return false };
            let rustc_middle::mir::StatementKind::Assign(pair) = &st.kind else { return false };
            let rustc_middle::mir::Rvalue::Use(rustc_middle::mir::Operand::Copy(src)) = &pair.1 else { return false };
            binding(pair.0.local) && e.end_places.iter().any(|p| p.local == src.local.index())
          });
          if copy_into_binding || represented.edges.contains(&(loan.loan, e.location, e.occurrence.successor)) {
            continue;
          }
          let initialized = |local| facts.rows.iter().flat_map(|r| &r.facts).any(|f|
            f.place_local == Some(local) && matches!(&f.fact, Fact::Init {path, ..} if path == *h)
              && dominates(body, f.location, e.location));
          // After it.next() a temporary carries the loan into the
          // switch. Its previous holder already crossed a HoldEnd.
          let handed_on = facts.rows.iter().flat_map(|r| &r.facts).any(|f| {
            matches!(&f.fact,Fact::HoldEnd{loan:id,holder,..} if *id==loan.loan && holder==*h)
              && f.occurrence.successor.is_some_and(|s| dominates(body, s, e.location))
          });
          // A skipped binding can still be the presentation label on the
          // end. Only draw an arrow from a named slot that carried this loan.
          let named = e.end_places.iter().filter(|p| &p.name == *h).map(|p| rustc_middle::mir::Local::from_usize(p.local));
          let mut named = named.filter(|&l| !binding(l));
          let from = if handed_on || !loan.holders.contains(h) {
            ResourceTy::Anonymous
          } else if let Some(local) = named.next().filter(|&l| named.next().is_none() && initialized(l)) {
            let Some(r) = self.raps.place(local, h) else { continue };
            if r.rap.is_ref() || !r.rap.holds_borrow() { continue; }
            ResourceTy::Value(r.rap.clone())
          } else {
            ResourceTy::Anonymous
          };
          let to = ResourceTy::Value(lender.clone());
          let id_ev = *self.unique_id;
          *self.unique_id += 1;
          let event = if loan.kind == "mut" {
            ExternalEvent::MutableDie {
              slot: id_ev,
              from: from.clone(),
              to,
              id: id_ev,
              still_borrowed_by: Vec::new(),
              of_ref_var: false,
              static_lender: None,
              exhaustion: Vec::new(),
            }
          } else {
            ExternalEvent::StaticDie {
              slot: id_ev,
              from: from.clone(),
              to,
              id: id_ev,
              still_borrowed_by: Vec::new(),
              of_ref_var: false,
              static_lender: None,
              exhaustion: Vec::new(),
            }
          };
          let mut arm_path = parent.clone();
          arm_path.push((id, a));
          if entry && (site.path == arm_path || !site.path.starts_with(&arm_path)) {
            site.path = arm_path;
            site.row = branch_type.get_start_end(a).0;
            site.before = arm.e_data.first().map(|(_, e)| e.get_id());
          }
          represented.edges.insert((loan.loan, e.location, e.occurrence.successor));
          self.end_loans.insert(id_ev, vec![loan.loan]);
          put.push((site.path, site.before, (site.row, event)));
          if from.extract_rap().is_some() {
            for (_, outer) in self.preprocessed_events.iter() {
              if self.end_loans.get(&outer.get_id()).is_some_and(|ls| ls.contains(&loan.loan))
                && matches!(outer, ExternalEvent::MutableDie{still_borrowed_by,..}|ExternalEvent::StaticDie{still_borrowed_by,..} if still_borrowed_by.is_empty())
                && endpoints(outer).0 == &from
                && endpoints(outer).1.extract_rap() == Some(lender)
              {
                remove.insert(outer.get_id());
              }
            }
          }
        }
      }
    }
    remove_from(self.preprocessed_events, self.event_line_map, &remove);
    for (p, b, e) in put {
      insert(self.preprocessed_events, self.event_line_map, &p, b, e, false);
    }
  }

  /// On a let-else's failed edge, the pattern's bindings haven't received
  /// a value. End the scrutinee loan there, or at its holder's last use
  /// within the else block, before the failed path drops the lender.
  pub(crate) fn place_let_else_ends(&mut self) {
    let Some(facts) = self.body_facts else { return };
    if self.let_else_branches.is_empty() {
      return;
    }
    let switches = self.branch_switches();
    let mut covered = self.represented_edges(&switches).edges;
    let mut put = Vec::new();
    for &id in &self.let_else_branches {
      let Some(bindings) = self.arm_bindings.get(&id) else { continue };
      let mut path = Vec::new();
      let Some((_, ExternalEvent::Branch { branches, branch_type, live_vars, .. })) =
        branch_at(self.preprocessed_events, id, &mut path)
      else {
        continue;
      };
      let vars: BTreeSet<_> = bindings.iter().filter_map(|(_, l)| facts.bindings.locals.get(l)).collect();
      let Some(els) = branches[0].source else { continue };
      path.push((id, 0));
      for loan in &facts.loans {
        if loan.source != "borrow" || !loan.holders.iter().any(|h| vars.contains(h)) {
          continue;
        }
        // An inline borrow depicted only by a matched pattern binding
        // has no drawn loan to return in else. A stored value or a
        // borrowed call result already carries its loan into the switch.
        if loan.via_call.is_empty() && loan.direct_holders.iter().all(|h| vars.contains(h)) {
          continue;
        }
        let Some(issue) = facts.rows.iter().flat_map(|r| &r.facts).find(|e|
          matches!(e.fact, Fact::Borrow { loan: id, .. } if id == loan.loan)) else { continue };
        let Some(lender) = self.raps.get(&loan.lender).map(|r| &r.rap) else { continue };
        if !live_vars.contains(lender) {
          continue;
        }
        for row in &facts.rows {
          for ev in &row.facts {
            let Fact::BorrowEnd { loan: n, holder, .. } = &ev.fact else { continue };
            // A may-loan from an earlier conditional or loop cannot be
            // returned unconditionally by the failed pattern.
            if *n != loan.loan || !dominates(&self.bwf.body, issue.location, ev.location) {
              continue;
            }
            let on_entry = facts.entered(ev.location, ev.occurrence.successor)
              .is_some_and(|arm| Some(arm) == branches[0].source);
            if !on_entry && !crate::facts::location_arms(self.tcx, &self.bwf.body, facts, ev.location).contains(&els) {
              continue;
            }
            let from = if vars.contains(holder) {
              ResourceTy::Anonymous
            } else if let Some(h) = self.raps.get(holder) {
              ResourceTy::Value(h.rap.clone())
            } else {
              continue;
            };
            let mut site = Site { path: path.clone(), row: row.line, before: None };
            if on_entry {
              site.row = branch_type.get_start_end(0).0;
              site.before = branches[0].e_data.first().map(|(_, e)| e.get_id());
            } else if let Fact::BorrowEnd { to_line, .. } = ev.fact {
              self.locate(&branches[0].e_data, &switches, ev.location, ev, to_line, &mut site);
            }
            if !covered.insert((loan.loan, ev.location, ev.occurrence.successor)) { continue; }
            let id = *self.unique_id;
            *self.unique_id += 1;
            self.end_loans.insert(id, vec![loan.loan]);
            let to = ResourceTy::Value(lender.clone());
            let end = if loan.kind == "mut" {
              ExternalEvent::MutableDie { slot: id, from, to, id, static_lender: None, still_borrowed_by: Vec::new(), of_ref_var: false, exhaustion: Vec::new() }
            } else {
              ExternalEvent::StaticDie { slot: id, from, to, id, static_lender: None, still_borrowed_by: Vec::new(), of_ref_var: false, exhaustion: Vec::new() }
            };
            put.push((site.path, site.before, (site.row, end)));
          }
        }
      }
    }
    for (path, before, ev) in put {
      insert(self.preprocessed_events, self.event_line_map, &path, before, ev, false);
    }
  }

  /// Returns true when the ending edge itself enters the selected arm.
  fn locate(
    &self,
    events: &Events,
    switches: &BTreeMap<usize, Vec<Location>>,
    at: Location,
    end: &Ev,
    next: usize,
    site: &mut Site,
  ) -> bool {
    let body = &self.bwf.body;
    let Some(facts) = self.body_facts else { return false };
    let entered = facts.entered(end.location, end.occurrence.successor);
    let path = crate::facts::location_arms(self.tcx, body, facts, at);
    for (_, e) in events {
      let ExternalEvent::Branch { id, branches, branch_type, split_point, merge_point, .. } = e else { continue };
      let sw = switches.get(id).map(Vec::as_slice).unwrap_or(&[]);
      if let Some(a) = entered.and_then(|arm| branches.iter().position(|b| b.source == Some(arm))) {
        site.path.push((*id, a));
        // An edge into an empty arm has no instruction to draw. Its
        // boundary is the arm's entry on the drawn fork.
        let (lo, _) = branch_type.get_start_end(a);
        let hi = branch_type.get_start_end(a).1;
        site.row = if lo <= next && next <= hi { next } else { lo.max(*split_point).min(*merge_point) };
        return true;
      }
      if !sw.is_empty() && sw.iter().all(|&s| end.occurrence.successor.is_some_and(|to| dominates(body, to, s))) {
        if site.row <= *split_point {
          site.before = Some(*id);
          return false;
        }
      }
      // A match's pattern comparison executes before either successor.
      // Its label can be drawn below the fork. Keep the effect at that
      // label so the legacy arm replay does not copy it into each body.
      if let Some(label) = facts.compared_pattern(body, at, end.occurrence.successor) {
        if let Some(i) = branches.iter().position(|arm| arm.source == Some(label)) {
          site.path.push((*id, i));
          return false;
        }
      }
      if let Some((i, arm)) = branches.iter().enumerate().find(|(_, arm)| arm.source.is_some_and(|s| path.contains(&s))) {
        site.path.push((*id, i));
        return self.locate(&arm.e_data, switches, at, end, next, site);
      }
    }
    false
  }
}

fn branch_at<'a>(events: &'a Events, wanted: usize, path: &mut Path) -> Option<&'a (usize, ExternalEvent)> {
  for event in events {
    let ExternalEvent::Branch { id, branches, .. } = &event.1 else { continue };
    if *id == wanted {
      return Some(event);
    }
    for (a, arm) in branches.iter().enumerate() {
      path.push((*id, a));
      if let Some(found) = branch_at(&arm.e_data, wanted, path) {
        return Some(found);
      }
      path.pop();
    }
  }
  None
}

/// Different copies can hold the same loan. Only a boundary of the
/// displayed holder can identify the template row being split.
fn nested_destinations(
  candidates: Vec<(&str, Site, bool, usize)>, holder: &str, root: Option<&str>, row: usize,
) -> Vec<((Site, bool), BTreeSet<usize>)> {
  let mut sites = BTreeMap::<_, BTreeSet<_>>::new();
  for (names, site, only_holder, loan) in candidates {
    if names.split('|').any(|n| n == holder || Some(n) == root) {
      sites.entry((site, only_holder)).or_default().insert(loan);
    }
  }
  if !sites.keys().any(|(s, _)| s.row == row) { return Vec::new(); }
  let mut sites: Vec<_> = sites.into_iter().collect();
  sites.sort_by_key(|((s, _), _)| s.row != row);
  sites
}

/// A split end has its own drawing ID on every path. Associate it with
/// the loans on that edge before any later placement pass can select it.
fn identify_end(event: &mut ExternalEvent, fresh: bool, loans: Vec<usize>, next: &mut usize, by_id: &mut BTreeMap<usize, Vec<usize>>) {
  let id = match event {
    ExternalEvent::StaticDie { id, .. } | ExternalEvent::MutableDie { id, .. } | ExternalEvent::RefDie { id, .. } => id,
    _ => unreachable!("only loan ends can be split"),
  };
  if fresh {
    *id = *next;
    *next += 1;
  }
  by_id.insert(*id, loans);
}

fn endpoints(e: &ExternalEvent) -> (&ResourceTy, &ResourceTy) {
  match e {
    ExternalEvent::RefDie { from, to, .. }
    | ExternalEvent::MutableDie { from, to, .. }
    | ExternalEvent::StaticDie { from, to, .. } => (from, to),
    _ => unreachable!("only loan end templates are placed"),
  }
}

fn dominates(body: &Body<'_>, at: Location, other: Location) -> bool {
  if at.block == other.block {
    at.statement_index <= other.statement_index
  } else {
    body.basic_blocks.dominators().dominates(at.block, other.block)
  }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Site {
  path: Path,
  before: Option<usize>,
  row: usize,
}

impl Site {
  fn matches(&self, events: &Events, path: &Path, row: usize, event: usize) -> bool {
    if &self.path != path || self.row != row { return false; }
    let Some(anchor) = self.before else { return true };
    if anchor == event { return true; }
    let Some(list) = events_at(events, path) else { return false };
    list.iter().take_while(|(_, e)| e.get_id() != anchor).any(|(_, e)| e.get_id() == event)
  }
}

fn events_at<'a>(mut events: &'a Events, path: &[(usize, usize)]) -> Option<&'a Events> {
  for &(id, a) in path {
    let (_, ExternalEvent::Branch { branches, .. }) = events.iter().find(|(_, e)| e.get_id() == id)? else { return None };
    events = &branches.get(a)?.e_data;
  }
  Some(events)
}

fn templates_in(
  events: &Events,
  path: &mut Path,
  loans: Option<&BTreeMap<usize, Vec<usize>>>,
  out: &mut Vec<(usize, ExternalEvent, Path)>,
) {
  for (line, e) in events {
    match e {
      ExternalEvent::Branch { id, branches, .. } => {
        for (i, arm) in branches.iter().enumerate() {
          path.push((*id, i));
          templates_in(&arm.e_data, path, loans, out);
          path.pop();
        }
      }
      ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. } | ExternalEvent::RefDie { .. }
        if loans.is_none_or(|by_id| by_id.get(&e.get_id()).is_some_and(|ids| !ids.is_empty())) =>
      {
        out.push((*line, e.clone(), path.clone()))
      }
      _ => {}
    }
  }
}

/// Changing only the holder preserves the event's position in both lists,
/// including an unstacked end on the same row as a function call.
fn replace_end(events: &mut Events, map: &mut LineMap, end: &ExternalEvent) {
  for (_, e) in events {
    if e.get_id() == end.get_id() {
      *e = end.clone();
    } else if let ExternalEvent::Branch { branches, .. } = e {
      for arm in branches { replace_end(&mut arm.e_data, &mut arm.line_map, end); }
    }
  }
  for e in map.values_mut().flatten().filter(|e| e.get_id() == end.get_id()) {
    *e = end.clone();
  }
}

fn remove_from(events: &mut Events, map: &mut LineMap, ids: &BTreeSet<usize>) {
  remove_except_reserved(events, map, ids, &BTreeSet::new());
}

// A slot can itself be inside a containing arm. A stacked arrow moved
// within its original row keeps its reservation in the enclosing line map.
fn remove_except_reserved(events: &mut Events, map: &mut LineMap, removed: &BTreeSet<usize>, reserved: &BTreeSet<usize>) {
    if removed.is_empty() {
        return;
    }
    events.retain(|(_, e)| !removed.contains(&e.get_id()));
    for arrows in map.values_mut() {
        arrows.retain(|e| !removed.contains(&e.get_id()) || reserved.contains(&e.get_id()));
    }
    for (_, e) in events {
        if let ExternalEvent::Branch { branches, .. } = e {
            for arm in branches {
                remove_except_reserved(&mut arm.e_data, &mut arm.line_map, removed, reserved);
            }
        }
    }
}


fn insert(
  events: &mut Events,
  map: &mut LineMap,
  path: &[(usize, usize)],
  before: Option<usize>,
  ev: (usize, ExternalEvent),
  stack: bool,
) {
  if let Some((&(id, arm), rest)) = path.split_first() {
    let Some((_, ExternalEvent::Branch { branches, live_vars, .. })) = events.iter_mut().find(|(_, e)| e.get_id() == id) else {
      return;
    };
    let (from, to) = endpoints(&ev.1);
    let giver = if matches!(from, ResourceTy::Index(..)) { None } else { from.extract_rap() };
    for rap in [giver, to.extract_rap()].into_iter().flatten() {
      if !rap.is_fn() && !branches.iter().any(|b| b.decl_vars.contains(rap)) {
        live_vars.insert(rap.clone());
      }
    }
    let b = &mut branches[arm];
    insert(&mut b.e_data, &mut b.line_map, rest, before, ev, stack);
    return;
  }
  let limit = before.and_then(|id| events.iter().position(|(_, e)| e.get_id() == id)).unwrap_or(events.len());
  let (from, to) = endpoints(&ev.1);
  let limit = events[..limit]
    .iter()
    .position(|(line, e)| {
      *line == ev.0 && e.is_gos_ev().is_some_and(|r| from.extract_rap() == Some(r) || to.extract_rap() == Some(r))
    })
    .unwrap_or(limit);
  let at = events[..limit].iter().rposition(|(l, _)| *l <= ev.0).map_or(0, |i| i + 1);
  if stack && ev.1.is_var_arrow_ev() {
    map.entry(ev.0).or_default().push(ev.1.clone());
  }
  events.insert(at, ev);
}

fn declared_at(
  events: &Events,
  r: &crate::svg_generator::data::ResourceAccessPoint,
  arms: &[crate::facts::SourceArm],
  path: &mut Path,
) -> bool {
  for (_, e) in events {
    let ExternalEvent::Branch { id, branches, .. } = e else { continue };
    for (a, branch) in branches.iter().enumerate() {
      if !branch.source.is_some_and(|s| arms.contains(&s)) {
        continue;
      }
      path.push((*id, a));
      if declared_at(&branch.e_data, r, arms, path) || branch.decl_vars.contains(r) {
        return true;
      }
      path.pop();
    }
  }
  false
}

fn insert_local(
  events: &mut Events,
  path: &[(usize, usize)],
  ev: (usize, ExternalEvent),
  stacked: bool,
  fork: Option<&crate::svg_generator::data::ResourceAccessPoint>,
) {
  let (&(id, a), rest) = path.split_first().unwrap();
  let Some((_, ExternalEvent::Branch { branches, live_vars, .. })) = events.iter_mut().find(|(_, e)| e.get_id() == id) else {
    return;
  };
  if let Some(r) = fork.filter(|r| !branches.iter().any(|b| b.decl_vars.contains(r))) {
    live_vars.insert(r.clone());
  }
  let arm = &mut branches[a];
  if !rest.is_empty() {
    return insert_local(&mut arm.e_data, rest, ev, stacked, fork);
  }
  let (from, to) = match &ev.1 {
    ExternalEvent::StaticDie { from, to, .. }
    | ExternalEvent::MutableDie { from, to, .. }
    | ExternalEvent::RefDie { from, to, .. } => (from.extract_rap(), to.extract_rap()),
    ExternalEvent::OrAltBorrow { into, ro, .. } => (Some(into), Some(ro)),
    _ => (None, None),
  };
  let mut at = arm.e_data.iter().rposition(|(l, _)| *l <= ev.0).map_or(0, |i| i + 1);
  if let Some(k) =
    arm.e_data[..at].iter().position(|(l, e)| *l == ev.0 && e.is_gos_ev().is_some_and(|r| Some(r) == from || Some(r) == to))
  {
    at = k;
  }
  if matches!(&ev.1, ExternalEvent::RefDie { .. }) {
    if let Some(i) = arm.e_data[..at].iter().rposition(|(l, e)| {
      let (f, t) = crate::svg_generator::data::ResourceAccessPoint_extract(e);
      *l == ev.0 && e.is_gos_ev().is_none() && from.is_some() && (f.extract_rap() == from || t.extract_rap() == from)
    }) {
      at = i + 1;
    }
  }
  if stacked && ev.1.is_var_arrow_ev() {
    arm.line_map.entry(ev.0).or_default().push(ev.1.clone());
  }
  arm.e_data.insert(at, ev);
}


#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_copy_cannot_validate_the_other_holders_template_row() {
    // let q = r; read(r); read(q); -- one loan, two last uses.
    let site = |row| Site { path: vec![(12, 0)], row, before: None };
    let edges = vec![("r", site(5), true, 0), ("q", site(6), false, 0)];
    assert!(nested_destinations(edges.clone(), "r", None, 6).is_empty());
    let own = nested_destinations(edges, "r", None, 5);
    assert_eq!(own.len(), 1);
    assert_eq!(own[0].0.0.row, 5);
    assert!(own[0].0.1);
  }

  #[test]
  fn split_repoint_ends_remain_selectable_by_their_own_loan() {
    // Two MIR definitions reaching an overwrite through different arms.
    // The later passes look up each copy by its drawing ID.
    let mut first = ExternalEvent::RefDie { slot: 12,
      from: ResourceTy::Anonymous, to: ResourceTy::Caller, id: 12, num_curr_borrowers: 0,
    };
    let mut second = first.clone();
    let mut by_id = BTreeMap::from([(12, vec![0, 1])]);
    let mut next = 13;
    identify_end(&mut first, false, vec![0], &mut next, &mut by_id);
    identify_end(&mut second, true, vec![1], &mut next, &mut by_id);
    assert_eq!(first.get_id(), 12);
    assert_eq!(second.get_id(), 13);
    assert_eq!(next, 14);
    assert_eq!(by_id[&first.get_id()], [0]);
    assert_eq!(by_id[&second.get_id()], [1]);
  }
}
