//! Map loan boundaries from the borrow-checking body into its drawn branches.
//! The source of an ending edge can precede the switch even when it is the
//! assignment in a successor that prompted the walk to draw a give-back.
use crate::expr_visitor::ExprVisitor;
use crate::facts::{Ev, Fact, LoanInfo};
use crate::facts::repoints::Boundary;
use crate::svg_generator::data::{BranchType, ExternalEvent, ResourceAccessPoint, ResourceTy, StaticLender};
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
  edges: BTreeSet<(usize, crate::facts::Occurrence)>,
  arm_loans: BTreeSet<(usize, usize, usize)>, // Branch id, arm, loan.
}

impl<'a, 'tcx> ExprVisitor<'a, 'tcx> {
  /// Place the iterator's release and the yielded reference's last use
  /// from their separate borrowck boundaries.
  pub(super) fn place_iterator_boundaries(
    &mut self, iter: &crate::facts::IteratorStorage, item: &ResourceAccessPoint,
    lender: &ResourceTy, loans: &[usize], exhaustion: &BTreeMap<Location, crate::svg_generator::data::IteratorExhaustion>,
  ) -> Option<BTreeSet<crate::facts::Occurrence>> {
    use crate::facts::repoints::occurrence_precedes;
    use crate::facts::DropRoute;
    use crate::svg_generator::hover_messages::loan_kind_word;
    use rustc_middle::mir::TerminatorKind;

    let (Some(facts), Some(local), [loan], Some(owner)) =
      (self.body_facts, item.ref_local(), loans, lender.extract_rap()) else { return None };
    let info = facts.loans.iter().find(|l| l.loan == *loan && l.source == "borrow")?;
    let kind = loan_kind_word(info)?;
    let place = self.fork_lender(info)?;
    if owner.is_ref() || !(iter.borrows_collection || iter.moves_references) || !iter.loans.contains(loan)
      || info.lender_local != owner.scope_local() || info.lender_local.is_none() {
      return None;
    }
    let body = &self.bwf.body;
    let switches = self.branch_switches();
    let in_slot = |end: Boundary<'_>, slot: rustc_middle::mir::Local| {
      matches!(end.fact.end_places.as_slice(), [p] if p.local == slot.index() && p.projection.is_empty())
    };
    let loop_release = |end: Boundary<'_>| {
      if !end.release || !in_slot(end, iter.storage) { return None; }
      let edge = exhaustion.get(&end.fact.occurrence.successor?)?;
      dominates(body, iter.next, end.fact.occurrence.at).then_some(edge)
    };
    let boundaries: Vec<_> = self.reference_effects.ends(*loan)
      .filter(|end| end.fact.drop_route == DropRoute::Normal
        && !facts.inline_holder_ends.iter().any(|(_, fact)| std::ptr::eq(fact, end.fact)))
      .collect();
    let mut exhaustions = BTreeMap::new();
    for end in boundaries.iter().copied() {
      if let Some(edge) = loop_release(end) {
        let (site, _) = self.normal_fork_site(facts, &switches, edge.closing_row, end.fact, edge.closing_row);
        exhaustions.insert(end.key(), site);
      }
    }
    if exhaustions.is_empty() { return None; }
    let branch_exhaustion = exhaustions.values().any(|site| !site.path.is_empty());
    let iterator_holder = |end: Boundary<'_>| end.fact.end_places.iter().any(|p|
      p.local == iter.storage.index() && p.projection.is_empty());
    // A release carried only by hidden storage needs a lender mark on
    // its actual leg. Where the loop shares the parent column, retain
    // the existing exit layout for releases with no unique drawn item.
    let can_place_early_release = |end: Boundary<'_>|
      in_slot(end, local) || branch_exhaustion && iterator_holder(end);
    if boundaries.iter().any(|&end| end.release && !exhaustions.contains_key(&end.key()) && !can_place_early_release(end)) {
      return None;
    }

    let mut releases = BTreeMap::<(Site, bool, bool), Vec<Boundary<'a>>>::new();
    let mut stops = BTreeMap::<Site, Vec<Boundary<'a>>>::new();
    for end in boundaries {
      let next = match end.fact.fact {
        Fact::BorrowEnd { to_line, .. } | Fact::HoldEnd { to_line, .. } => to_line,
        _ => continue,
      };
      if let Some(site) = exhaustions.remove(&end.key()) {
        releases.entry((site, true, false)).or_default().push(end);
      } else if in_slot(end, local) || end.release && can_place_early_release(end) {
        let (site, entry) = self.normal_fork_site(facts, &switches, end.row, end.fact, next);
        if !entry && end.fact.location == body.terminator_loc(end.fact.location.block)
          && matches!(body[end.fact.location.block].terminator().kind, TerminatorKind::SwitchInt { .. })
        {
          // No drawn leg represents this successor.
          continue;
        }
        if end.release {
          releases.entry((site, false, in_slot(end, local))).or_default().push(end);
        } else {
          // A child stored on this path, including in another traversal,
          // can still return to the item.
          // A child issued on a sibling leg does not delay this stop.
          let awaits_child = facts.loans.iter().any(|child| child.lender_local == Some(local)
            && child.made_through.contains(loan)
            && (!child.direct_locals.is_empty() || !child.holder_stores.is_empty()
              || facts.iterators.iter().any(|nested| nested.loans.contains(&child.loan)))
            && child.issue.is_some_and(|issue| dominates(body, issue, end.fact.occurrence.at))
            && !self.reference_effects.ends(child.loan).any(|edge| edge.release
              && occurrence_precedes(body, edge.fact.occurrence, end.fact.occurrence.at)));
          if !awaits_child {
            stops.entry(site).or_default().push(end);
          }
        }
      }
    }

    for ((site, exhausted, from_item), ends) in releases {
      if exhausted && self.drawn_references.ends.iter().any(|(id, placed)|
        (self.row_layout.iterator_end_notes.contains_key(id) || self.row_layout.aggregate_titles.contains_key(id))
          && ends.iter().all(|end| placed.iter().any(|old| old.key() == end.key())))
      {
        continue;
      }
      let id = *self.unique_id;
      *self.unique_id += 1;
      let from = if from_item { ResourceTy::Value(item.clone()) } else { ResourceTy::Anonymous };
      let to = lender.clone();
      let event = if kind == "mutable" {
        ExternalEvent::MutableDie { slot: id, from, to, id, static_lender: None, still_borrowed_by: Vec::new(),
          of_ref_var: false, exhaustion: Vec::new() }
      } else {
        ExternalEvent::StaticDie { slot: id, from, to, id, static_lender: None, still_borrowed_by: Vec::new(),
          of_ref_var: false, exhaustion: Vec::new() }
      };
      if exhausted {
        let title = if iter.moves_references {
          format!("When the iterator runs out of items, the borrow checker no longer restricts access to {place} through this iterator.")
        } else {
          format!("When the iterator runs out of items, its {kind} borrow of {place} ends. This borrow no longer restricts access to {place}")
        };
        if ends.iter().all(|end| facts.usable_after_release.contains(&end.key())) {
          self.row_layout.iterator_end_notes.insert(id, title);
        } else {
          self.row_layout.aggregate_titles.insert(id, title);
        }
      } else {
        self.row_layout.aggregate_titles.insert(id,
          format!("The iterator's {kind} borrow of {place} ends on this path. This borrow no longer restricts access to {place}."));
      }
      self.insert_iterator_end(site, event, ends);
    }
    let mut stop_edges = BTreeSet::new();
    for (site, ends) in stops {
      let id = *self.unique_id;
      *self.unique_id += 1;
      let event = ExternalEvent::RefDie { slot: id, from: ResourceTy::Value(item.clone()), to: ResourceTy::Anonymous,
        num_curr_borrowers: 1, id };
      if iter.borrows_collection {
        self.row_layout.iterator_item_stops.insert(id);
      }
      stop_edges.extend(ends.iter().map(|end| end.fact.occurrence));
      self.insert_iterator_end(site, event, ends);
    }
    Some(stop_edges)
  }

  fn insert_iterator_end(&mut self, site: Site, event: ExternalEvent, ends: Vec<Boundary<'a>>) {
    let id = event.get_id();
    if ends.iter().all(|end| end.fact.occurrence == ends[0].fact.occurrence) {
      self.row_layout.record_arm_point(id, ends[0].fact);
    }
    self.end_loans.insert(id, ends.iter().map(|end| end.loan).collect::<BTreeSet<_>>().into_iter().collect());
    self.drawn_references.ends.insert(id, ends);
    insert(self.preprocessed_events, self.event_line_map, &site.path, site.before, (site.row, event), false);
  }

  /// A projected pattern's reference and the loan of the scrutinee's
  /// owner can end independently. Match each drawn endpoint to its MIR
  /// edge before describing what ends.
  pub(crate) fn attribute_projected_pattern_ends(&mut self) {
    use crate::svg_generator::hover_messages::{loan_kind_word, styled_name_html};
    use rustc_middle::mir::{BorrowKind, ProjectionElem, Rvalue, StatementKind};

    let Some(facts) = self.body_facts else { return };
    let switches = self.branch_switches();
    let mut slots = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), None, &mut slots);
    let mut proven = Vec::new();
    for (line, event, path) in &slots {
      let (ExternalEvent::StaticDie { slot, id, static_lender: None, still_borrowed_by, of_ref_var: false, exhaustion, .. }
        | ExternalEvent::MutableDie { slot, id, static_lender: None, still_borrowed_by, of_ref_var: false, exhaustion, .. }) = event
      else { continue };
      if !still_borrowed_by.is_empty() || !exhaustion.is_empty() { continue; }
      let (from, to) = endpoints(event);
      let Some(holder_rap) = from.extract_rap() else { continue };
      let Some(holder) = holder_rap.ref_local() else { continue };
      let Some(owner) = to.extract_rap().and_then(|rap| rap.owner_local()) else { continue };
      let body = &self.bwf.body;
      let mut stores = facts.rows.iter().flat_map(|row| &row.facts).filter(|ev| {
        ev.place_local == Some(holder) && matches!(ev.fact, Fact::Init { .. })
      });
      let Some(store) = stores.next().filter(|_| stores.next().is_none()) else { continue };
      let Some(StatementKind::Assign(assign)) = body[store.location.block].statements
        .get(store.location.statement_index).map(|s| &s.kind) else { continue };
      let Rvalue::Ref(_, kind, place) = assign.1 else { continue };
      if assign.0.as_local() != Some(holder)
        || !place.projection.iter().any(|p| matches!(p, ProjectionElem::ConstantIndex { .. } | ProjectionElem::Subslice { .. }))
      {
        continue;
      }
      let mutable = matches!(kind, BorrowKind::Mut { .. });
      let mut boundaries = facts.rows.iter().flat_map(|row| row.facts.iter().map(move |e| (row.line, e)))
        .filter_map(|(row, ev)| {
          let (loan, next, release) = match ev.fact {
            Fact::BorrowEnd { loan, to_line, .. } => (loan, to_line, true),
            Fact::HoldEnd { loan, to_line, .. } if ev.occurrence.at == store.location && !ev.continuing_places.is_empty() =>
              (loan, to_line, false),
            _ => return None,
          };
          if ev.drop_route != crate::facts::DropRoute::Normal || !dominates(body, store.location, ev.occurrence.at)
            || self.end_loans.get(id).is_some_and(|ids| ids.as_slice() != [loan])
          { return None; }
          let [holder_place] = ev.end_places.as_slice() else { return None };
          if holder_place.local != holder.index() || !holder_place.projection.is_empty() { return None; }
          let (_, issued) = self.bwf.borrow_set.location_map().get_index(loan)?;
          if issued.borrowed_place().as_local() != Some(owner) { return None; }
          let mut site = Site { path: Vec::new(), before: None, row };
          self.locate(self.preprocessed_events, &switches, ev.display_location, ev, next, &mut site);
          if !release && site.path != *path {
            // Pattern binding stores can be scoped to the if-let
            // condition even though only its successful edge runs them.
            let arms: Vec<_> = facts.switches.iter().filter_map(|exit| {
              crate::event_order::edge_dominates_use(&body.basic_blocks, exit.edge.switch,
                exit.edge.successor.start_location(), store.location)
                .then_some(crate::facts::SourceArm { branch: exit.branch?, index: exit.arm? })
            }).collect();
            let mut declaration = Vec::new();
            if !declared_at(self.preprocessed_events, holder_rap, &arms, &mut declaration)
              || !declaration.starts_with(&site.path) { return None; }
            site.path = declaration;
          }
          // A let-else draws its successful binding after the fork.
          // Keep the release on that binding's dot when the same
          // store dominates the release on the matched edge.
          if site.path != *path {
            let [(id, arm)] = site.path.strip_prefix(path.as_slice())? else { return None };
            if !release || !self.let_else_branches.contains(id)
              || !self.arm_bindings.get(id).is_some_and(|bindings| bindings.contains(&(*arm, holder)))
            {
              return None;
            }
            site.path.clone_from(path);
            site.before = None;
            site.row = *line;
          }
          if release && site.row != *line { return None; }
          Some((Boundary { loan, fact: ev, release, row }, site))
        });
      let Some((edge, site)) = boundaries.next().filter(|_| boundaries.next().is_none()) else { continue };
      if slots.iter().filter(|(_, other, other_path)| other_path == path
        && endpoints(other).0 == from).count() != 1 { continue; }
      let Some(info) = facts.loans.iter().find(|l| l.loan == edge.loan) else { continue };
      let Some(kind) = loan_kind_word(info) else { continue };
      let holder = styled_name_html(&from.name());
      let place = styled_name_html(&to.name());
      let title = if !edge.release {
        format!("{holder} has no further use. Another reference keeps the borrow live.")
      } else {
        let projected = if mutable { "mutable" } else { "shared" };
        format!("{holder} holds a {projected} reference into the matched value. The {kind} borrow of {place} ends here. This borrow no longer restricts access to {place}.")
      };
      let end = if !edge.release {
        ExternalEvent::RefDie { slot: *slot, from: from.clone(), to: ResourceTy::Anonymous, num_curr_borrowers: 0, id: *id }
      } else if kind == "mutable" {
        ExternalEvent::MutableDie { slot: *slot, from: from.clone(), to: to.clone(), id: *id, static_lender: None,
          still_borrowed_by: Vec::new(), of_ref_var: false, exhaustion: Vec::new() }
      } else {
        ExternalEvent::StaticDie { slot: *slot, from: from.clone(), to: to.clone(), id: *id, static_lender: None,
          still_borrowed_by: Vec::new(), of_ref_var: false, exhaustion: Vec::new() }
      };
      let reordered = edge.release && !site.matches(self.preprocessed_events, path, *line, *id);
      proven.push((end, edge, site, title, reordered));
    }
    for (event, edge, site, title, reordered) in proven {
      let id = event.get_id();
      self.row_layout.aggregate_titles.entry(id).or_insert(title);
      self.end_loans.insert(id, vec![edge.loan]);
      self.drawn_references.ends.insert(id, vec![edge]);
      if edge.release {
        replace_end(self.preprocessed_events, self.event_line_map, &event);
        if reordered {
          let ids = BTreeSet::from([id]);
          remove_except_reserved(self.preprocessed_events, self.event_line_map, &ids, &ids);
          insert(self.preprocessed_events, self.event_line_map, &site.path, site.before, (site.row, event), false);
        }
      } else {
        remove_from(self.preprocessed_events, self.event_line_map, &BTreeSet::from([id]));
        insert(self.preprocessed_events, self.event_line_map, &site.path, site.before, (site.row, event), false);
      }
    }
  }

  /// A shared reference staged into an unused field can have region ends
  /// on both edges of a let-else because the aggregate is used. The field
  /// itself never carries the reference beyond its initialization in the
  /// drawing. Prove this from place uses, including uses of the enclosing
  /// aggregate, before shortening the field's line.
  pub(crate) fn unused_shared_field_copy(
    &self, holder: &crate::svg_generator::data::ResourceAccessPoint, loan: usize,
  ) -> Option<(usize, &'a LoanInfo, &'a crate::facts::EndPlace)> {
    use rustc_middle::mir::{Local, Place, ProjectionElem, Statement, StatementKind};
    use rustc_middle::mir::visit::{PlaceContext, Visitor};

    if self.let_else_branches.is_empty() || !holder.is_member() || holder.is_mutref() {
      return None;
    }
    let facts = self.body_facts?;
    let info = facts.loans.iter().find(|info| info.loan == loan && info.kind == "shared" && info.source == "untracked")?;
    if !info.made_through.iter().any(|parent| facts.loans.iter().any(|l| l.loan == *parent && l.source == "caller")) {
      return None;
    }
    let [store] = info.holder_stores.as_slice() else { return None };
    let local = Local::from_usize(store.place.local);
    if !store.complete || store.place.projection.is_empty()
      || !store.place.projection.iter().all(|p| matches!(p, crate::facts::EndProjection::Field(_)))
      || self.raps.place(local, &store.place.name).is_none_or(|r| &r.rap != holder)
    {
      return None;
    }
    let body = &self.bwf.body;
    if body.local_decls[local].ty.needs_drop(self.tcx, body.typing_env(self.tcx)) {
      return None;
    }
    let ends = facts.rows.iter().flat_map(|row| &row.facts).filter(|e| {
      matches!(e.fact, Fact::BorrowEnd { loan: id, .. } if id == loan) && e.end_places.contains(&store.place)
    });
    if ends.take(2).count() < 2 {
      return None;
    }
    struct Uses<'p> {
      store: Location,
      local: Local,
      projection: &'p [crate::facts::EndProjection],
      used: bool,
    }
    impl<'tcx> Visitor<'tcx> for Uses<'_> {
      fn visit_statement(&mut self, statement: &Statement<'tcx>, at: Location) {
        if !matches!(statement.kind, StatementKind::FakeRead(_) | StatementKind::PlaceMention(_)) {
          self.super_statement(statement, at);
        }
      }

      fn visit_place(&mut self, place: &Place<'tcx>, context: PlaceContext, at: Location) {
        if at == self.store || place.local != self.local || !context.is_use() {
          return;
        }
        // An aggregate, the field, and everything reached through the
        // field all use the stored reference. Sibling fields do not.
        self.used |= self.projection.iter().zip(place.projection.iter()).all(|(slot, used)| {
          matches!((slot, used), (crate::facts::EndProjection::Field(a), ProjectionElem::Field(b, _)) if *a == b.index())
        });
      }
    }
    let mut uses = Uses { store: store.occurrence.at, local, projection: &store.place.projection, used: false };
    uses.visit_body(body);
    if uses.used {
      return None;
    }
    let row = facts.rows.iter().find(|row| row.facts.iter().any(|ev| ev.occurrence == store.occurrence))?;
    Some((row.line, info, &store.place))
  }

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
    let switches = self.branch_switches();
    let mut destinations = BTreeMap::new();
    for (line, e) in self.preprocessed_events.iter() {
      if self.row_layout.iterator_item_stops.contains(&e.get_id()) { continue; }
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
        let path = paths.pop_first().unwrap();
        let before = self.drawn_references.ends.get(&e.get_id()).and_then(|ends| {
          let mut anchors = ends.iter().map(|end| {
            let next = match end.fact.fact {
              Fact::BorrowEnd { to_line, .. } | Fact::HoldEnd { to_line, .. } => to_line,
              _ => return None,
            };
            let (site, _) = self.normal_fork_site(facts, &switches, end.row, end.fact, next);
            (site.path == path).then_some(site.before).flatten()
          });
          let anchor = anchors.next().flatten()?;
          (switches.get(&anchor).is_some_and(|tests| tests.len() > 1)
            && matches!(branch_at(self.preprocessed_events, anchor, &mut Vec::new()),
              Some((_, ExternalEvent::Branch { branch_type: BranchType::If(..), .. })))
            && anchors.all(|id| id == Some(anchor))).then_some(anchor)
        });
        destinations.insert(e.get_id(), (path, before));
      }
    }
    let mut moved = Vec::new();
    self.preprocessed_events.retain(|(line, e)| {
      let Some((path, before)) = destinations.get(&e.get_id()) else { return true };
      moved.push((path, *before, (*line, e.clone())));
      false
    });
    for (path, before, ev) in moved {
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
      if let Some(anchor) = before {
        self.row_layout.end_before_fork.insert(ev.1.get_id(), anchor);
        let id = ev.1.get_id();
        if let Some(ends) = self.drawn_references.ends.get(&id).filter(|ends| !ends.is_empty() && ends.iter().all(|end| end.release)) {
          // A release can share a row with another occurrence of the
          // same loan (including a residual return before binding).
          let mut releases = ends.clone();
          for end in ends {
            for other in self.reference_effects.ends(end.loan).filter(|other| other.release && other.row == end.row) {
              if !releases.iter().any(|e| e.fact.occurrence == other.fact.occurrence && e.loan == other.loan) {
                releases.push(other);
              }
            }
          }
          if let Some(title) = self.edge_release_title(facts, &ev.1, &releases) {
            self.row_layout.joined_end_titles.insert(id, title);
          }
        }
      }
      insert_local(self.preprocessed_events, path, before, ev, in_arm_stack || stacked, fork.as_ref());
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
      let synthesized = ev.get_id() >= self.first_synthesized_end;
      if synthesized {
        if walk.iter().any(|(row, other, path)| {
          *row == line
            && *path == origin
            && std::mem::discriminant(other) == std::mem::discriminant(&ev)
            && endpoints(other) == endpoints(&ev)
            && self.end_loans.get(&other.get_id()) == Some(&ids)
        }) {
          remove.insert(ev.get_id());
          continue;
        }
        if !matches!(ev, ExternalEvent::MutableDie { .. }) || ids.len() != 1 || !origin.is_empty() {
          continue;
        }
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
      let mut boundaries = BTreeMap::<Site, Vec<crate::facts::repoints::Boundary>>::new();
      let mut ends_before_new_borrow = false;
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
          // A later overwrite in an arm may have emitted the end of an
          // earlier copy from the caller. The transfer chain names the
          // copied slot and its true edge precedes the fork.
          let caller_copy = matches!(to, ResourceTy::Caller) && info.source == "caller"
            && edge.transfer_paths.iter().any(|path| path.len() > 1 && path.last().is_some_and(|step| self.raps.get(&holder)
              .is_some_and(|binding| from.extract_rap() == Some(&binding.rap) && binding.local.is_some_and(|l| l.index() == step.place.local))))
            && origin.first().and_then(|(id, _)| switches.get(id)).is_some_and(|at|
              !at.is_empty() && at.iter().all(|&fork| dominates(&self.bwf.body, edge.location, fork)));
          let lender = caller_copy || info.lender == drawn
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
          let (mut site, entry) = if synthesized {
            self.normal_fork_site(facts, &switches, row.line, edge, *next)
          } else {
            let mut site = Site { path: Vec::new(), before: None, row: row.line };
            let entry = self.locate(self.preprocessed_events, &switches, edge.display_location, edge, *next, &mut site);
            (site, entry)
          };
          if synthesized && !entry {
            // A pattern store can be shown on the scrutinee's row even though
            // the store and its end execute only on the successful edge.
            while let Some((id, arm)) = events_at(self.preprocessed_events, &site.path).and_then(|events| {
              let mut matches = events.iter().filter_map(|(_, event)| {
                let ExternalEvent::Branch { id, branches, .. } = event else { return None };
                let mut candidates = self.arm_bindings.get(id)?.iter().filter_map(|&(arm, local)| {
                  edge.exact_end_places.iter().any(|p| p.local == local.index() && p.projection.is_empty()).then_some(arm)
                });
                let arm = candidates.next()?;
                if candidates.any(|other| other != arm) { return None; }
                let source = branches.get(arm)?.source?;
                facts.switches.iter().any(|switch| switch.branch == Some(source.branch) && switch.arm == Some(source.index)
                  && dominates(&self.bwf.body, switch.edge.successor.start_location(), edge.location)).then_some((*id, arm))
              });
              let selected = matches.next()?;
              matches.next().is_none().then_some(selected)
            }) {
              site.path.push((id, arm));
              site.before = None;
            }
          }
          if synthesized && !is_own && entry {
            if let Some(&(id, a)) = site.path.last() {
              let Some((_, ExternalEvent::Branch { branches, .. })) =
                branch_at(self.preprocessed_events, id, &mut Vec::new()) else { continue };
              let arm = &branches[a];
              // A shared access to this same place in the successor arm
              // needs the exclusive loan to have ended on entry.
              let borrowed_place = |loan: &LoanInfo| {
                self.bwf.borrow_set.location_map().get(&loan.issue?).map(|data| data.borrowed_place())
              };
              if let Some(place) = borrowed_place(info) {
                ends_before_new_borrow |= facts.loans.iter().any(|other| {
                  other.kind == "shared" && borrowed_place(other).is_some_and(|shared| {
                    shared.local == place.local && (shared.projection.starts_with(place.projection)
                      || place.projection.starts_with(shared.projection))
                  })
                    && other.issue.is_some_and(|at| {
                      arm.source.is_some_and(|s| crate::facts::location_arms(self.tcx, &self.bwf.body, facts, at).contains(&s))
                        && edge.occurrence.successor.is_some_and(|to| dominates(&self.bwf.body, to, at))
                    })
                });
              }
              site.before = arm.e_data.first().map(|(_, e)| e.get_id());
            }
          }
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
          boundaries.entry(site.clone()).or_default().push(crate::facts::repoints::Boundary {
            loan: *loan, fact: edge, release: !is_own, row: row.line,
          });
          if synthesized {
            // The region can end on a failed scrutinee before its
            // success-only binding receives the reference. Prove the
            // holder at this edge independently of the drawn arm.
            let bound = self.raps.get(&holder).filter(|rd| from.extract_rap() == Some(&rd.rap))
              .and_then(|rd| rd.local)
              .is_some_and(|local| edge.end_places.iter().any(|p| p.local == local.index() && p.name == holder));
            if !bound { lender_only.insert(site.clone()); }
          }
          sites.entry(site).or_default().insert(*loan);
        }
      }
      if sites.is_empty() { continue; }
      if synthesized {
        if !ends_before_new_borrow || sites.len() < 2 || sites.keys().any(|s| s.path.is_empty())
          || !facts.loans.iter().any(|l| l.loan == ids[0] && l.holders == [holder.as_str()]) {
          continue;
        }
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
      let individual = dropped || synthesized || site.before.is_some() && sites.len() > 1;
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
        let ends = if individual { boundaries.remove(&site).unwrap_or_default() }
          else { boundaries.values().flatten().copied().collect() };
        if !detached && !individual && site.matches(self.preprocessed_events, &origin, line, ev.get_id()) {
          self.drawn_references.ends.insert(ev.get_id(), ends);
          continue;
        }
        remove.insert(ev.get_id());
        let mut e = ev.clone();
        identify_end(&mut e, !first, ids, self.unique_id, &mut self.end_loans);
        let directed = synthesized && individual && site.path.len() > 1
          && matches!(ends.as_slice(), [end] if end.release);
        if let Some(anchor) = site.before {
          self.row_layout.end_before_fork.insert(e.get_id(), anchor);
        }
        if detached {
          if let ExternalEvent::StaticDie { from, .. } | ExternalEvent::MutableDie { from, .. } = &mut e {
            *from = ResourceTy::Anonymous;
          }
        }
        if directed {
          if let Some(title) = self.edge_release_title(facts, &e, &ends) {
            self.row_layout.joined_end_titles.insert(e.get_id(), title);
          }
        }
        self.drawn_references.ends.insert(e.get_id(), ends);
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
      if !matches!(ev, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) {
        continue;
      }
      let Some(r) = from.extract_rap() else { continue };
      let loans = &self.end_loans[&ev.get_id()];
      let &[loan_id] = loans.as_slice() else { continue };
      if let Some(local) = r.ref_local().filter(|_| !r.is_mutref()) {
        let belongs = |p: &crate::facts::EndPlace| p.local == local.index() && p.projection.is_empty();
        let boundaries: Vec<_> = self.reference_effects.ends(loan_id).collect();
        let mut own = Vec::new();
        let mut tested_release = false;
        let mut has_handoff = false;
        let mut has_other = false;
        let mut complete = true;
        let mut sibling_placements = BTreeMap::new();
        for boundary in &boundaries {
          let end = boundary.fact;
          if end.end_places.iter().any(belongs) {
            let next = match end.fact {
              Fact::BorrowEnd { to_line, .. } | Fact::HoldEnd { to_line, .. } => to_line,
              _ => continue,
            };
            let (site, entry) = self.normal_fork_site(facts, &switches, boundary.row, end, next);
            complete &= site.path.starts_with(path) && !site.path.iter().any(|&(id, _)|
              matches!(branch_at(self.preprocessed_events, id, &mut Vec::new()),
                Some((_, ExternalEvent::Branch { branch_type: BranchType::Loop(..), .. }))));
            // The bypass releases the loan after the decision has read
            // through this holder. That is distinct from copying out of
            // a value already stored before the fork.
            if boundary.release && entry {
              tested_release |= facts.loans.iter().any(|child| child.lender_local == Some(local)
                && child.made_through.contains(&loan_id)
                && self.reference_effects.ends(child.loan).any(|end_of_test|
                  end_of_test.release && end_of_test.fact.occurrence.successor == Some(end.location)));
            }
            if !boundary.release {
              let body = &self.bwf.body;
              let transfers = body[end.location.block].statements.get(end.location.statement_index).is_some_and(|stmt| {
                use rustc_middle::mir::{Operand, ProjectionElem, Rvalue, StatementKind};
                let StatementKind::Assign(pair) = &stmt.kind else { return false };
                match &pair.1 {
                  Rvalue::Use(Operand::Copy(place)) => place.as_local() == Some(local),
                  Rvalue::Ref(_, _, place) => place.local == local && place.projection.first() == Some(&ProjectionElem::Deref),
                  _ => false,
                }
              });
              has_handoff |= transfers;
            }
            own.push((site, *boundary));
          } else if boundary.release {
            // Another slot can name the reaching holder itself, or a
            // derived loan that ends at the same MIR boundary.
            let covered = slots.iter().any(|(row, other, path)| {
              if !matches!(other, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) {
                return false;
              }
              let (giver, lender) = endpoints(other);
              let Some(giver) = giver.extract_rap().filter(|g| *g != r && lender == to) else { return false };
              if remove.contains(&other.get_id()) { return false; }
              let carries = |p: &crate::facts::EndPlace| self.raps.place(
                rustc_middle::mir::Local::from_usize(p.local), &p.name).is_some_and(|slot| &slot.rap == giver);
              let ids = &self.end_loans[&other.get_id()];
              ids.iter().copied().any(|id| {
                if id != loan_id && !facts.loans.iter().any(|l| l.loan == id && l.made_through.contains(&loan_id)) {
                  return false;
                }
                let releases_here = |candidate: Boundary<'_>| {
                  candidate.release && candidate.fact.end_places.iter().any(carries)
                };
                let releases = || self.reference_effects.ends(id).filter(|&candidate| releases_here(candidate));
                let drawn_site = |boundary: Boundary<'_>| {
                  let Fact::BorrowEnd { to_line, .. } = boundary.fact.fact else { unreachable!() };
                  self.normal_fork_site(facts, &switches, boundary.row, boundary.fact, to_line).0
                };
                releases().filter(|candidate| candidate.fact.occurrence == end.occurrence).any(|candidate| {
                  let site = drawn_site(candidate);
                  let other_id = other.get_id();
                  let assigned = self.drawn_references.ends.get(&other_id);
                  let there = |site: &Site| site.matches(self.preprocessed_events, path, *row, other_id);
                  let at_one_release = assigned.is_none_or(|ends| ends.iter().all(|e|
                    e.release && e.fact.occurrence == candidate.fact.occurrence));
                  if there(&site) && assigned.map_or_else(
                    || releases().all(|other| other.fact.occurrence == end.occurrence || !there(&drawn_site(other))),
                    |_| at_one_release,
                  ) {
                    return true;
                  }
                  // A single legacy slot may stand for releases on
                  // several arms. Place a release at the proven edge
                  // before letting it cover that arm.
                  if !site.path.starts_with(path)
                    || !ids.iter().all(|&id| id == loan_id || id == candidate.loan)
                    || assigned.is_some_and(|ends| ends.iter().any(|e|
                      !e.release || !releases().any(|b| b.fact.occurrence == e.fact.occurrence)))
                    || releases().any(|b| !boundaries.iter().any(|parent|
                      parent.release && parent.fact.occurrence == b.fact.occurrence))
                    || slots.iter().any(|(_, e, _)| e.get_id() != other_id
                      && matches!(e, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. })
                      && endpoints(e).0.extract_rap() == Some(giver) && endpoints(e).1 == to
                      && self.end_loans.get(&e.get_id()).is_some_and(|ids| ids.contains(&id)))
                  {
                    return false;
                  }
                  let keep_original = releases().any(|end| there(&drawn_site(end))
                    && assigned.is_none_or(|ends| ends.iter().all(|e| e.fact.occurrence == end.fact.occurrence)));
                  let (_, ends, _, _) = sibling_placements.entry((other_id, site))
                    .or_insert_with(|| (other.clone(), Vec::new(), keep_original, path.clone()));
                  ends.push(candidate);
                  if candidate.loan != boundary.loan {
                    ends.push(*boundary);
                  }
                  true
                })
              })
            });
            complete &= covered;
            has_other |= covered;
          }
        }
        if complete && tested_release && has_handoff && has_other {
          let loan = facts.loans.iter().find(|l| l.loan == loan_id).unwrap();
          let kind = crate::svg_generator::hover_messages::loan_kind_word(loan);
          let lender = self.fork_lender(loan);
          remove.insert(ev.get_id());
          for ((id, site), (mut event, ends, keep_original, origin)) in sibling_placements {
            let new_id = keep_original || !remove.insert(id);
            let loans = self.end_loans[&id].clone();
            if !new_id {
              let (from, _) = endpoints(&event);
              if let Some(field) = from.extract_rap().filter(|r| r.is_member()) {
                // A field's reference can be copied on the sibling
                // arm. Its holding ends there even though this release
                // now belongs to a different arm.
                let mut handoffs = BTreeMap::<Site, Vec<Boundary<'_>>>::new();
                for boundary in loans.iter().flat_map(|&loan| self.reference_effects.ends(loan)) {
                  let Fact::HoldEnd { to_line, .. } = boundary.fact.fact else { continue };
                  if !boundary.fact.end_places.iter().any(|p| self.raps.place(
                    rustc_middle::mir::Local::from_usize(p.local), &p.name).is_some_and(|slot| &slot.rap == field))
                  {
                    continue;
                  }
                  let (destination, _) = self.normal_fork_site(facts, &switches, boundary.row, boundary.fact, to_line);
                  if destination.path.starts_with(&origin) && destination.path != site.path {
                    handoffs.entry(destination).or_default().push(boundary);
                  }
                }
                for (destination, ends) in handoffs {
                  let mut handoff = ExternalEvent::RefDie {
                    slot: event.end_slot().unwrap(), from: from.clone(), to: ResourceTy::Anonymous,
                    id, num_curr_borrowers: 1,
                  };
                  let loans = ends.iter().map(|b| b.loan).collect::<BTreeSet<_>>();
                  identify_end(&mut handoff, true, loans.into_iter().collect(), self.unique_id, &mut self.end_loans);
                  self.drawn_references.ends.insert(handoff.get_id(), ends);
                  put.push((destination, handoff));
                }
              }
            }
            identify_end(&mut event, new_id, loans, self.unique_id, &mut self.end_loans);
            let id = event.get_id();
            let first = ends[0].fact;
            if ends.iter().all(|e| e.fact.occurrence == first.occurrence) {
              self.row_layout.record_arm_point(id, first);
            }
            if let Some(title) = self.edge_release_title(facts, &event, &ends) {
              self.row_layout.joined_end_titles.insert(id, title);
            }
            self.drawn_references.ends.insert(id, ends);
            put.push((site, event));
          }
          for (site, boundary) in own {
            let mut placed = if boundary.release { ev.clone() } else {
              ExternalEvent::RefDie { slot: ev.end_slot().unwrap(), from: from.clone(),
                to: ResourceTy::Anonymous, id: ev.get_id(), num_curr_borrowers: 1 }
            };
            identify_end(&mut placed, true, vec![loan_id], self.unique_id, &mut self.end_loans);
            let id = placed.get_id();
            self.row_layout.record_arm_point(id, boundary.fact);
            self.drawn_references.ends.insert(id, vec![boundary]);
            if boundary.release {
              if let Some(title) = self.edge_release_title(facts, &placed, &[boundary]) {
                self.row_layout.joined_end_titles.insert(id, title);
              }
            } else if let (Some(kind), Some(lender)) = (kind, &lender) {
              if let Some((transfer, title)) = self.shared_aggregate_reborrows.get(&boundary.fact.occurrence.at) {
                self.row_layout.aggregate_titles.insert(*transfer, title.clone());
              }
              let holder = crate::svg_generator::hover_messages::styled_name_html(r.name());
              self.row_layout.holder_handoff_titles.insert(id, format!(
                "{holder} stops using its reference. The {kind} borrow of {lender} remains live."));
            }
            put.push((site, placed));
          }
          continue;
        }
      }
      if !path.is_empty() { continue; }
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
      let mut mine = BTreeMap::<_, Vec<_>>::new();
      let mut others = BTreeSet::new();
      for (row, e, h, next) in ends {
        let mut site = Site { path: Vec::new(), before: None, row };
        self.locate(self.preprocessed_events, &switches, e.location, e, next, &mut site);
        if h == r.name() {
          mine.entry(site).or_default().push(Boundary { loan: *loan, fact: e, release: true, row });
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
      if others.is_empty() || mine.is_empty() || mine.keys().any(|s| s.path.is_empty() || others.contains(&s.path)) {
        continue;
      }
      // A later exit pass handles a conditional that reaches its join.
      // If all paths leave here the last holder on each exit is final.
      if !mine.keys().all(|s| {
        s.path.last().is_some_and(|(id, _)| {
          matches!(branch_at(self.preprocessed_events,*id,&mut Vec::new()),
          Some((_, ExternalEvent::Branch { branches, .. })) if branches.iter().all(|b|b.diverges))
        })
      }) {
        continue;
      }
      remove.insert(ev.get_id());
      for (site, edges) in mine {
        let mut copy = ev.clone();
        if let ExternalEvent::StaticDie { id, .. } | ExternalEvent::MutableDie { id, .. } = &mut copy {
          *id = *self.unique_id;
          *self.unique_id += 1;
          self.end_loans.insert(*id, vec![*loan]);
          self.drawn_references.ends.insert(*id, edges);
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
        self.drawn_references.ends.insert(id, vec![Boundary { loan: *loan, fact: e, release: false, row: *row }]);
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
        // A split caller return may already be in its leaf arm, on
        // entry. If a reborrow is the last use of the caller's loan,
        // both ends belong at the reborrow's actual ending edge.
        if matches!(to, ResourceTy::Caller) {
          let ids = self.end_loans.get(&ev.get_id()).map(Vec::as_slice);
          if let Some([parent]) = ids {
            let mut ends = BTreeSet::new();
            for (child_row, child_event) in &arm.e_data {
              if *child_row <= line || !matches!(child_event, ExternalEvent::StaticDie { .. } | ExternalEvent::MutableDie { .. }) {
                continue;
              }
              let Some([child]) = self.end_loans.get(&child_event.get_id()).map(Vec::as_slice) else { continue };
              if !facts.loans.iter().any(|l| l.loan == *child && l.made_through.contains(parent) && l.through.as_ref() == Some(r.name())) {
                continue;
              }
              let Some(row) = facts.rows.iter().find(|row| row.line == *child_row) else { continue };
              if row.facts.iter().any(|e| matches!(e.fact, Fact::BorrowEnd { loan, .. } if loan == *parent)
                && arm.source.is_some_and(|s| crate::facts::location_arms(self.tcx, &self.bwf.body, facts, e.location).contains(&s))
                && row.facts.iter().any(|c| matches!(c.fact, Fact::BorrowEnd { loan, .. } if loan == *child)
                  && c.location == e.location && c.occurrence.successor == e.occurrence.successor)) {
                ends.insert(*child_row);
              }
            }
            if ends.len() == 1 {
              remove.insert(ev.get_id());
              put.push((origin, None, (*ends.first().unwrap(), ev), false));
            }
          }
        }
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
              && lender.is_some_and(|r| l.columns.contains(r.name()) || &l.lender == r.name())
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
      let mut handoffs = BTreeMap::new();
      let mut skipped_let_else = false;
      let let_else_local = r.ref_local().filter(|&local|
        self.row_layout.let_else_bindings.contains_key(&(r.get_owner(), local)));
      let mut bound_ends = Vec::new();
      let mut sibling_returns = BTreeSet::new();
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
        if let_else_local.is_some_and(|local| e.end_places.iter().any(|place|
          place.local == local.index() && place.projection.is_empty()))
        {
          bound_ends.push(Boundary { loan: *loan, fact: e, release: !own, row });
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
        // Later tests of a let chain run only after the earlier pattern
        // matched. The renderer puts their call events on that leg. A
        // discriminant edge bypassing those tests belongs on the failed
        // leg, even if both successors have the same displayed row.
        for (_, branch) in &arm.e_data {
          let ExternalEvent::Branch { id, branches, .. } = branch else { continue };
          let Some(cond) = self.event_spans.get(id) else { continue };
          let [tested, failed] = branches.as_slice() else { continue };
          let Some((failure_row, _)) = failed.e_data.iter().find(|(_, e)| matches!(e, ExternalEvent::ChainFailUse { .. })) else { continue };
          let span = self.bwf.body.source_info(e.location).span;
          if !cond.contains(span) { continue; }
          let body = &self.bwf.body;
          if switches.get(id).is_some_and(|ss| ss.contains(&e.location)) {
            let Some(to) = e.occurrence.successor else { continue };
            let skips_use = facts.rows.iter().flat_map(|r| &r.facts).any(|later|
              matches!(&later.fact, Fact::BorrowEnd { loan: l, holder: name, .. } if l == loan && name == h)
                && later.location != e.location && cond.contains(body.source_info(later.location).span)
                && body[e.location.block].terminator().successors().any(|b| b != to.block
                  && body.basic_blocks.dominators().dominates(b, later.location.block)));
            if skips_use {
              site = Site { path: vec![(*id, 1)], row: *failure_row, before: None };
            }
          } else if site.path.is_empty() {
            let call = tested.e_data.iter().find(|(_, event)| {
              matches!(event, ExternalEvent::PassByStaticReference { .. } | ExternalEvent::PassByMutableReference { .. })
                && self.event_spans.get(&event.get_id()).is_some_and(|s| cond.contains(*s) && (s.contains(span) || span.contains(*s)))
            });
            if let Some((row, _)) = call {
              site = Site { path: vec![(*id, 0)], row: *row, before: None };
            }
          }
        }
        // A pattern can yield several copies of the same loan. rustc
        // ends the unused binding's holding before the fork, while its
        // sibling continues to carry the loan into the fork.
        if own && h == holder && site.path.is_empty() && site.before.is_some()
          && e.end_places.iter().any(|p| facts.bindings.columns.get(holder).is_some_and(|ls|
            ls.len() == 1 && ls[0].index() == p.local)) {
          handoffs.insert((site.clone(), *loan, e.occurrence.successor), Boundary { loan: *loan, fact: e, release: !own, row });
        }
        if !own && h != holder && row == line && !site.path.is_empty()
          && e.end_places.iter().any(|p| p.name != *holder && h.split('|').any(|name| name == p.name)) {
          sibling_returns.insert((*loan, site.path[0].0, e.location));
        }
        let Some(&(nested, _)) = site.path.first() else { continue };
        // The binding is drawn after this fork. The scrutinee's end on
        // failure cannot be used to move or discard the binding's own end.
        if let_else_local.is_some_and(|local| site.path.iter().any(|(branch, _)| {
          self.let_else_branches.contains(branch) && self.arm_bindings.get(branch)
            .is_some_and(|bindings| bindings.iter().any(|&(_, binding)| binding == local))
        })) {
          skipped_let_else = true;
          continue;
        }
        let Some((_, ExternalEvent::Branch { branches: arms, live_vars, branch_type, .. })) =
          arm.e_data.iter().find(|(_, e)| e.get_id() == nested)
        else {
          continue;
        };
        let returns_caller = matches!(to, ResourceTy::Caller) && !own && facts.loans.iter().any(|child|
          child.made_through.contains(loan) && child.through.as_ref() == Some(holder)
            && facts.rows.iter().flat_map(|r| &r.facts).any(|end| end.location == e.location
              && end.occurrence.successor == e.occurrence.successor
              && matches!(&end.fact, Fact::BorrowEnd { loan, holder, .. } if *loan == child.loan && holder == h)));
        if matches!(branch_type, crate::svg_generator::data::BranchType::Loop(..))
          || arms.iter().any(|b| b.decl_vars.contains(r))
          || (!returns_caller && !live_vars.contains(r) && !lender.is_some_and(|l| live_vars.contains(l)))
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
        candidates.push((if returns_caller { holder } else { h.as_str() }, site, own && !is_own, Boundary { loan: *loan, fact: e, release: !own, row }));
      }
      let mut dests = nested_destinations(candidates, holder, root.map(String::as_str), line);
      if dests.is_empty() && handoffs.len() == 1 {
        let ((site, loan, successor), boundary) = handoffs.pop_first().unwrap();
        // This replaces the whole template. A release on a different
        // fork or for another reaching loan cannot account for it.
        if sibling_returns.iter().any(|&(returned, fork, at)| {
          sibling_completes_handoff(&loans, loan, site.before, returned, fork)
            && successor.is_some_and(|s| dominates(&self.bwf.body, s, at))
        }) {
          dests.push(((site, true), vec![boundary]));
        }
      }
      if dests.is_empty() {
        if skipped_let_else {
          if bound_ends.is_empty() {
            remove.insert(ev.get_id());
          } else {
            self.drawn_references.ends.insert(ev.get_id(), bound_ends);
          }
        }
        continue;
      }
      let listed = arm.line_map.get(&line).is_some_and(|g| g.iter().any(|x| x.get_id() == ev.get_id()));
      remove.insert(ev.get_id());
      for (i, ((s, only_holder), edges)) in dests.into_iter().enumerate() {
        let mut event = if only_holder {
          ExternalEvent::RefDie { slot: ev.end_slot().unwrap_or(ev.get_id()), from: from.clone(), to: to.clone(), id: ev.get_id(), num_curr_borrowers: 1 }
        } else {
          ev.clone()
        };
        let loans = edges.iter().map(|b| b.loan).collect::<BTreeSet<_>>();
        identify_end(&mut event, i > 0, loans.into_iter().collect(), self.unique_id, &mut self.end_loans);
        self.drawn_references.ends.insert(event.get_id(), edges);
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
      let mut dest = BTreeMap::<_, Vec<_>>::new();
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
          dest.entry((site, holder.clone())).or_default().push(Boundary { loan: *loan, fact: end, release: true, row: row.line });
        }
      }
      if dest.keys().all(|(site, holder)| site.matches(self.preprocessed_events, &origin, line, ev.get_id()) && holder == current.name()) {
        self.drawn_references.ends.insert(ev.get_id(), dest.into_values().flatten().collect());
        continue;
      }
      let stay = dest.len() == 1 && dest.keys().next().is_some_and(|(site, _)| site.matches(self.preprocessed_events, &origin, line, ev.get_id()));
      if !stay { remove.insert(ev.get_id()); }
      for (i, ((site, name), boundaries)) in dest.into_iter().enumerate() {
        let mut e = ev.clone();
        let (id, from) = match &mut e {
          ExternalEvent::StaticDie { id, from, .. } | ExternalEvent::MutableDie { id, from, .. } => (id, from),
          _ => continue,
        };
        if i > 0 {
          *id = *self.unique_id;
          *self.unique_id += 1;
        }
        self.end_loans.insert(*id, boundaries.iter().map(|b| b.loan).collect());
        self.drawn_references.ends.insert(*id, boundaries);
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

  /// End placement records the actual boundaries it drew. Pattern arms
  /// also need to know that their binding received the loan: the failing
  /// successor of the switch can release it without ever initializing that
  /// local. A row shared by the two successors says nothing about coverage.
  fn drawn_ends(&self, switches: &BTreeMap<usize, Vec<Location>>) -> RepresentedEnds {
    let Some(facts) = self.body_facts else { return Default::default() };
    let mut slots = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), None, &mut slots);
    let mut drawn = RepresentedEnds::default();
    for (line, event, path) in slots {
      if !matches!(&event, ExternalEvent::MutableDie { still_borrowed_by, .. } | ExternalEvent::StaticDie { still_borrowed_by, .. }
        if still_borrowed_by.is_empty()) { continue; }
      let id = event.get_id();
      let (from, to) = endpoints(&event);
      let holder = from.extract_rap().and_then(|r| r.ref_local().or_else(|| r.owner_local()));
      let matches_lender = |loan| facts.loans.iter().find(|l| l.loan == loan).is_some_and(|l|
        l.lender_local.and_then(|local| self.raps.place(local, &l.lender)).is_some_and(|r| Some(&r.rap) == to.extract_rap()));
      let bound: BTreeSet<_> = if let Some(loans) = self.end_loans.get(&id) {
        loans.iter().copied().collect()
      } else {
        path.iter().flat_map(|(id, arm)| self.arm_bindings.get(id).into_iter().flatten()
          .filter(move |(a, p)| a == arm && Some(*p) == holder))
          .flat_map(|(_, p)| facts.binding_loans.get(p).into_iter().flat_map(|inits| inits.values().flatten().copied())).collect()
      };
      for loan in bound.into_iter().filter(|&l| matches_lender(l)) {
        drawn.arm_loans.extend(path.iter().map(|&(id, a)| (id, a, loan)));
      }
      if let Some(boundaries) = self.drawn_references.ends.get(&id) {
        for end in boundaries.iter().filter(|e| e.release) {
          let Fact::BorrowEnd { to_line, .. } = end.fact.fact else { continue };
          let mut site = Site { path: Vec::new(), row: end.row, before: None };
          self.locate(self.preprocessed_events, switches, end.fact.display_location, end.fact, to_line, &mut site);
          if site.matches(self.preprocessed_events, &path, line, id) {
            drawn.edges.insert(end.key());
          }
        }
      }
    }
    drawn
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
    let mut represented = self.drawn_ends(&switches);
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
          if copy_into_binding || represented.edges.contains(&(loan.loan, e.occurrence)) {
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
          // The failure note marks the failed test on a chain's skipped
          // path, even when the implicit else has no body or source row.
          if entry && arm.source_span.is_none() {
            if let Some((row, note)) = arm.e_data.iter().find(|(_, event)|
              matches!(event, ExternalEvent::ChainFailUse { ro, .. } if ro == lender)) {
              site.row = *row;
              site.before = Some(note.get_id());
            }
          }
          represented.edges.insert((loan.loan, e.occurrence));
          self.drawn_references.ends.insert(id_ev, vec![crate::facts::repoints::Boundary {
            loan: loan.loan, fact: e, release: true, row: *row,
          }]);
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

  /// A named reference already carries this loan when the pattern is
  /// tested, and the failure path contains its last use.
  pub(crate) fn let_else_has_outer_end(&self, arm: crate::facts::SourceArm) -> bool {
    let Some(facts) = self.body_facts else { return false };
    let body = &self.bwf.body;
    let tests = facts.tests_of(arm.branch);
    if tests.is_empty() { return false; }
    facts.rows.iter().flat_map(|row| &row.facts).any(|end| {
      let Fact::BorrowEnd { loan, .. } = end.fact else { return false };
      !end.end_places.is_empty()
        && facts.loans.iter().find(|info| info.loan == loan).and_then(|info| info.issue)
          .is_some_and(|issue| tests.iter().all(|&test| dominates(body, issue, test)))
        && crate::facts::location_arms(self.tcx, body, facts, end.location).contains(&arm)
    })
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
    let mut covered = self.drawn_ends(&switches).edges;
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
        // The successor containing the initialization of the pattern
        // binding is the successful path.
        let binds: Vec<_> = facts.rows.iter().flat_map(|r| &r.facts).filter_map(|ev| match &ev.fact {
          Fact::Init { path, .. } if vars.contains(path) && loan.holders.contains(path) => Some(ev.location),
          _ => None,
        }).collect();
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
              // The region summary can name a copy made only after the
              // pattern matched. That copy never held the scrutinee on
              // the failed edge. Keep the lender's return there.
              let stores: Vec<_> = facts.rows.iter().flat_map(|r| &r.facts)
                .filter(|e| h.local.is_some() && e.place_local == h.local && matches!(&e.fact, Fact::Init { path, .. } if path == holder))
                .map(|e| e.location).collect();
              if on_entry && ev.end_places.is_empty() && !stores.is_empty() && stores.iter().all(|&store|
                binds.iter().any(|&init| dominates(&self.bwf.body, init, store))) {
                ResourceTy::Anonymous
              } else {
                ResourceTy::Value(h.rap.clone())
              }
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
            if !covered.insert((loan.loan, ev.occurrence)) { continue; }
            let id = *self.unique_id;
            *self.unique_id += 1;
            self.end_loans.insert(id, vec![loan.loan]);
            self.drawn_references.ends.insert(id, vec![Boundary { loan: loan.loan, fact: ev, release: true, row: row.line }]);
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

fn sibling_completes_handoff(template: &[usize], held: usize, before: Option<usize>, returned: usize, fork: usize) -> bool {
  template == [held] && returned == held && before == Some(fork)
}

/// Different copies can hold the same loan. Only a boundary of the
/// displayed holder can identify the template row being split.
fn nested_destinations<T>(
  candidates: Vec<(&str, Site, bool, T)>, holder: &str, root: Option<&str>, row: usize,
) -> Vec<((Site, bool), Vec<T>)> {
  let mut sites = BTreeMap::<_, Vec<_>>::new();
  for (names, site, only_holder, loan) in candidates {
    if names.split('|').any(|n| n == holder || Some(n) == root) {
      sites.entry((site, only_holder)).or_default().push(loan);
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
  before: Option<usize>,
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
    return insert_local(&mut arm.e_data, rest, before, ev, stacked, fork);
  }
  let (from, to) = match &ev.1 {
    ExternalEvent::StaticDie { from, to, .. }
    | ExternalEvent::MutableDie { from, to, .. }
    | ExternalEvent::RefDie { from, to, .. } => (from.extract_rap(), to.extract_rap()),
    ExternalEvent::OrAltBorrow { into, ro, .. } => (Some(into), Some(ro)),
    _ => (None, None),
  };
  let limit = before.and_then(|id| arm.e_data.iter().position(|(_, e)| e.get_id() == id))
    .unwrap_or(arm.e_data.len());
  let mut at = arm.e_data[..limit].iter().rposition(|(l, _)| *l <= ev.0).map_or(0, |i| i + 1);
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
  fn sibling_return_proves_only_its_own_loan_at_its_own_fork() {
    // copy and last in progen_1906_edge_holders can both carry the
    // loans of a and b. An end of one cannot consume a two-loan slot.
    assert!(!sibling_completes_handoff(&[0, 1], 0, Some(4), 1, 4));
    assert!(!sibling_completes_handoff(&[0, 1], 0, Some(4), 0, 4));
    assert!(!sibling_completes_handoff(&[0], 0, Some(4), 0, 7));
    assert!(sibling_completes_handoff(&[0], 0, Some(4), 0, 4));
  }

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

#[cfg(test)]
mod boundary_tests;
