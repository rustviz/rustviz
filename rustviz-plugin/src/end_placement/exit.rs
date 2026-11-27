//! Additional loan and holder ends on diverging paths.
use super::{branch_at, dominates, events_at, insert, remove_except_reserved, templates_in, Path, Site};
use crate::expr_visitor::{borrow_end_of as ends, exhaustion_at, touches, ExprVisitor};
use crate::facts::{BodyFacts, EndPlace, Ev, Fact, HolderStep};
use crate::svg_generator::data::{BranchType, ExternalEvent, ResourceAccessPoint, ResourceTy};
use rustc_middle::mir::TerminatorKind;
use std::collections::{BTreeMap, BTreeSet, HashSet};

struct EndEdge<'a> {
  fact: &'a Ev,
  loan: usize,
  site: Site,
  own: bool,
  entry: bool,
  from_fork: bool,
  diverges: bool,
}

type Template = (usize, ExternalEvent, Path);

impl ExprVisitor<'_, '_> {
  pub(crate) fn place_exit_ends(&mut self) {
    let Some(facts) = self.body_facts else { return };
    let mut slots = Vec::new();
    templates_in(self.preprocessed_events, &mut Vec::new(), None, &mut slots);
    if slots.is_empty() || self.branch_conditions.is_empty() {
      return;
    }
    let edges = self.exit_edges(facts, &slots);
    let mut remove = BTreeSet::new();
    let mut insertions = Vec::new();
    let mut reserved = BTreeSet::new();
    let mut copied_loans = BTreeMap::new();
    let mut placed = BTreeSet::new();
    for (row, ev, orig) in &slots {
      let Some((r, to)) = ends(ev) else { continue };
      if to.extract_rap().is_some_and(|l| l.is_fn()) {
        continue;
      }
      let Some(slot_id) = ev.end_slot() else { continue };
      let Some(loans) = self.end_loans.get(&ev.get_id()) else {
        continue;
      };
      // Last visible end on each drawn leg. An undisplayed conditional
      // break cannot make its fall-through leg release the lender early.
      let mut sites = BTreeMap::<(Path, EndPlace, usize), Option<&EndEdge>>::new();
      let exhaustion = exhaustion_at(ev, *row).next().is_some();
      let own_slot = matches!(ev, ExternalEvent::RefDie { .. });
      let mut split_ends = BTreeMap::new();
      for loan in loans {
        let Some(loan_edges) = edges.get(loan) else { continue };
        // Column labels can be reused in different scopes. Resolve
        // this original slot to one MIR local for this loan.
        let candidates: BTreeSet<_> = loan_edges
          .iter()
          .flat_map(|edge| {
            let e = edge.fact;
            e.end_places
              .iter()
              .chain(e.caller_slot.iter())
              .chain(e.transfer_paths.iter().flatten().map(|step| &step.place))
          })
          .filter(|p| &p.name == r.name())
          .collect();
        let mut candidates = candidates.into_iter();
        let Some(slot) = candidates.next() else { continue };
        if candidates.next().is_some() {
          continue;
        }
        // A return in one arm can end a loan through its new holder;
        // the switch into its continuing sibling can end the very same
        // loan through the old holder. Both are alternatives to the
        // template after the join. Require every whole-loan boundary to
        // belong to this fork, including the end on the continuing edge.
        let whole: Vec<_> = loan_edges.iter().filter(|e| !e.own && !e.from_fork).collect();
        let split = whole.iter().find(|e| !e.diverges && e.entry && e.site.path.starts_with(orig)
          && e.site.path.len() == orig.len() + 1).map(|e| e.site.path[orig.len()].0)
          .filter(|id| !own_slot && !exhaustion && whole.iter().any(|e| e.diverges)
            && whole.iter().all(|e| e.site.path.starts_with(orig) && e.site.path.len() == orig.len() + 1
              && e.site.path[orig.len()].0 == *id && (e.diverges || e.entry)));
        if let Some(id) = split {
          split_ends.insert(*loan, (id, whole));
        }
        for edge in loan_edges {
          let EndEdge {
            fact,
            ref site,
            own,
            diverges,
            ..
          } = *edge;
          if (!diverges && (split.is_none() || !edge.entry || own)) || !site.path.starts_with(orig) || site.path == *orig {
            continue;
          }
          // A loan already returned at the condition keeps its
          // ordinary end. Only the separate exhaustion template
          // needs copies of a whole-loan end on these exit legs.
          if edge.from_fork && !own && !exhaustion {
            continue;
          }
          let giver =
            if fact.end_places.contains(slot) || fact.caller_slot.as_ref() == Some(slot) && matches!(to, ResourceTy::Caller) {
              slot
            } else {
              let [source] = fact.end_places.as_slice() else { continue };
              if own || own_slot {
                continue;
              }
              if source.name == "<iterator>" {
                if !fact.transfer_paths.iter().any(|p| connects(p, source, slot)) {
                  continue;
                }
                slot
              } else {
                // On the early exit the move to the normal return
                // slot hasn't executed. Use its reaching source.
                let transferred = loan_edges
                  .iter()
                  .any(|edge| edge.fact.transfer_paths.iter().any(|p| connects(p, source, slot)));
                let has_template = slots.iter().any(|(_, e, _)| {
                  ends(e).is_some_and(|(from, l)| from.name() == &source.name && l == to)
                    && self.end_loans.get(&e.get_id())
                      .is_some_and(|ids| ids.contains(loan))
                });
                if !transferred || has_template {
                  continue;
                }
                source
              }
            };
          let list = events_at(self.preprocessed_events, &site.path).unwrap();
          if list.iter().any(
            |(_, e)| matches!(e,ExternalEvent::OrAltBorrow { into,ro,end:true,.. } if into == r && Some(ro) == to.extract_rap()),
          ) {
            continue;
          }
          let (_, ExternalEvent::Branch { branches, .. }) =
            branch_at(self.preprocessed_events, site.path[orig.len()].0, &mut Vec::new()).unwrap()
          else {
            continue;
          };
          let branch_arm = site.path[orig.len()].1;
          // A slot declared on this very leg already has its normal
          // end there. Only a deeper exit needs its own copy.
          if branches.iter().any(|arm| arm.decl_vars.contains(r))
            && (!branches[branch_arm].decl_vars.contains(r)
              || site.path.len() == orig.len() + 1 && !branches.iter().all(|arm| arm.diverges))
          {
            continue;
          }
          // A holder ending the same row as a named transferee gets
          // its own end when the transferee gives the loan back.
          let handed_on = own
            && loan_edges.iter().any(|other| {
              !other.own
                && site.path == other.site.path
                && other
                  .fact
                  .end_places
                  .iter()
                  .any(|p| other.fact.transfer_paths.iter().any(|route| connects(route, giver, p)))
            });
          if own && !own_slot && !handed_on && !item_end(fact, giver, to) {
            continue;
          }
          let key = (site.path.clone(), giver.clone(), *loan);
          use std::collections::btree_map::Entry;
          match sites.entry(key) {
            Entry::Vacant(v) => {
              v.insert(Some(edge));
            }
            Entry::Occupied(mut e) => {
              let Some(prev) = e.get() else { continue };
              if (prev.own && !own) || (prev.own == own && dominates(&self.bwf.body, prev.fact.location, fact.location)) {
                e.insert(Some(edge));
              } else if prev.own == own
                && site.row != prev.site.row
                && !dominates(&self.bwf.body, fact.location, prev.fact.location)
              {
                // Distinct MIR paths share this drawn leg. It
                // has no single release point for these ends.
                e.insert(None);
              }
            }
          }
        }
      }
      // Only validated destinations (including a duplicate of this very
      // loan) cover a path. A nested end covers its leg, never its sibling.
      let mut covered = BTreeMap::<usize, BTreeMap<Path, bool>>::new();
      let mut copies = BTreeMap::new();
      for ((_, holder, _), edge) in sites.into_iter().filter_map(|(k, e)| Some((k, e?))) {
        let Some(giver) = self.raps.get(&holder.name).map(|r| r.rap.clone()) else { continue };
        let path = &edge.site.path;
        *covered.entry(edge.loan).or_default().entry(path.clone()).or_default() |= edge.entry;
        if slots.iter().any(|(_, e, p)| {
          p == path && ends(e) == Some((&giver, to))
            && self.end_loans.get(&e.get_id()).is_some_and(|ids| ids.contains(&edge.loan))
        }) || !placed.insert((slot_id, edge.loan, edge.fact.location, edge.fact.end_successor, path.clone(), holder.clone())) {
          continue;
        }
        // One drawn release can stand for several loans only at the
        // same boundary, with the same holder and the same end kind.
        copies.entry((path.clone(), holder.clone(), edge.fact.location, edge.fact.end_successor, edge.own))
          .or_insert_with(|| (giver, holder, edge, Vec::new())).3.push(edge.loan);
      }
      let list = events_at(self.preprocessed_events, orig).unwrap();
      let before = list.iter().position(|(_, e)| e.get_id() == ev.get_id()).unwrap();
      let move_slot = !exhaustion && loans.iter().all(|loan| covered.get(loan).is_some_and(|paths| {
        if covers_paths(&list[..before], orig, r, paths, false) { return true; }
        let Some((id, edges)) = split_ends.get(loan) else { return false };
        let Some((_, ExternalEvent::Branch { branches, .. })) = list[..before].iter().find(|(_, e)| e.get_id() == *id) else {
          return false;
        };
        branches.iter().enumerate().all(|(a, _)| {
          let mut path = orig.clone();
          path.push((*id, a));
          paths.contains_key(&path) || edges.iter().any(|edge| edge.site.path == path && slots.iter().any(|(_, e, p)|
            *p == path && ends(e).is_some_and(|(from, lender)| lender == to
              && edge.fact.end_places.iter().any(|h| &h.name == from.name()))
              && self.end_loans.get(&e.get_id()).is_some_and(|ids| ids.contains(loan))))
        })
      }));
      if move_slot {
        remove.insert(ev.get_id());
      }
      let mut reused = false;
      for (giver, holder, edge, ids) in copies.into_values() {
        let mut site = edge.site.clone();
        let own = edge.own;
        let stack = move_slot && site.row == *row && !reused;
        let newid = if stack {
          reused = true;
          reserved.insert(ev.get_id());
          ev.get_id()
        } else {
          let id = *self.unique_id;
          *self.unique_id += 1;
          id
        };
        copied_loans.insert(newid, ids);
        let project_item = item_end(edge.fact, &holder, to);
        let mut copy = ev.clone();
        if own && !own_slot && !project_item {
          copy = ExternalEvent::RefDie {
            slot: slot_id,
            from: ResourceTy::Value(giver),
            to: to.clone(),
            num_curr_borrowers: 0,
            id: newid,
          };
        } else {
          match &mut copy {
            ExternalEvent::StaticDie { id, from, .. }
            | ExternalEvent::MutableDie { id, from, .. }
            | ExternalEvent::RefDie { id, from, .. } => {
              *from = ResourceTy::Value(giver);
              *id = newid;
            }
            _ => unreachable!(),
          }
        }
        if site.before.is_none() {
          let list = events_at(self.preprocessed_events, &site.path).unwrap();
          let after_use = list
            .iter()
            .rposition(|(line, e)| *line == site.row && !edge.entry && touches(e, ends(&copy).unwrap().0));
          let anchor = list
            .iter()
            .skip(after_use.map_or(0, |i| i + 1))
            .find(|(line, _)| *line >= site.row);
          site.before = anchor.map(|(_, e)| e.get_id());
        }
        insertions.push((site, copy, stack && self.event_line_map.get(row).is_some_and(|v| v.len() > 1)));
      }
    }
    self.end_loans.extend(copied_loans);
    remove_except_reserved(self.preprocessed_events, self.event_line_map, &remove, &reserved);
    for (site, ev, stack) in insertions {
      insert(
        self.preprocessed_events,
        self.event_line_map,
        &site.path,
        site.before,
        (site.row, ev),
        stack,
      );
    }
  }
  /// Locate each compiler boundary once. Keep normal-path edges too:
  /// their transfers identify the holder that reaches an earlier exit.
  fn exit_edges<'a>(&self, facts: &'a BodyFacts, slots: &[Template]) -> BTreeMap<usize, Vec<EndEdge<'a>>> {
    let switches = self.branch_switches();
    let mut exhaustion_loops = HashSet::new();
    for (row, event, _) in slots {
      if let Some(ids) = self.end_loans.get(&event.get_id()) {
        for (target, _) in exhaustion_at(event, *row) {
          exhaustion_loops.extend(ids.iter().map(|&id| (id, target)));
        }
      }
    }
    let mut edges = BTreeMap::<usize, Vec<_>>::new();
    for (row, ev) in facts
      .rows
      .iter()
      .flat_map(|r| r.facts.iter().map(move |e| (r.line, e)))
      .chain(facts.inline_holder_ends.iter().map(|(r, e)| (*r, e)))
    {
      let (loan, next, own) = match &ev.fact {
        Fact::BorrowEnd { loan, to_line, .. } => (*loan, *to_line, false),
        Fact::HoldEnd { loan, to_line, .. } => (*loan, *to_line, true),
        _ => continue,
      };
      if ev.end_places.is_empty() && ev.caller_slot.is_none() {
        continue;
      }
      let mut site = Site {
        path: Vec::new(),
        row,
        before: None,
      };
      let entry = self.locate(
        self.preprocessed_events,
        &switches,
        self.site_span(ev.display_location),
        ev,
        next,
        &mut site,
      );
      let diverges = site.path.iter().any(|&(id, a)| {
        matches!(
            branch_at(self.preprocessed_events, id, &mut Vec::new()),
            Some((_, ExternalEvent::Branch { branches, branch_type, .. }))
                if !matches!(branch_type, BranchType::Loop(..)) && branches[a].diverges
        )
      });
      // A switch inside an arm can lead to a labeled break and a
      // fall-through that continues using the loan. Only a drawn
      // fork can display a release specific to a switch edge.
      let bb = &self.bwf.body[ev.location.block];
      if !entry
        && ev.location.statement_index == bb.statements.len()
        && matches!(bb.terminator().kind, TerminatorKind::SwitchInt { .. })
        && !site.path.is_empty()
      {
        continue;
      }
      // An item can expire while evaluating the condition, before
      // its switch. If every leg leaves this iteration, the drawn
      // loop has a separate exit on each leg and one for exhaustion.
      let group: &mut Vec<_> = edges.entry(loan).or_default();
      let fork = site.before.filter(|id| {
        let target = self.exit_scopes.get(id).and_then(|s| s.loop_id);
        target.is_some_and(|target| exhaustion_loops.contains(&(loan, target)))
          && switches
            .get(id)
            .is_some_and(|sw| sw.iter().any(|at| at.block == ev.location.block))
      });
      if let Some((
        _,
        ExternalEvent::Branch {
          id,
          branches,
          branch_type,
          ..
        },
      )) = fork.and_then(|id| branch_at(self.preprocessed_events, id, &mut Vec::new()))
      {
        if branches.iter().all(|arm| arm.diverges) {
          for a in 0..branches.len() {
            let mut path = site.path.clone();
            path.push((*id, a));
            let leg = Site {
              path,
              before: None,
              row: branch_type.get_start_end(a).0,
            };
            group.push(EndEdge {
              fact: ev,
              loan,
              site: leg,
              own,
              entry: true,
              from_fork: true,
              diverges: true,
            });
          }
        }
      }
      group.push(EndEdge {
        fact: ev,
        loan,
        site,
        own,
        entry,
        from_fork: false,
        diverges,
      });
    }
    edges
  }
}

// A branch that leaves cannot use coverage after the join. Working
// backwards also checks early exits inside a continuing arm. An end
// on the incoming switch edge covers the arm before any nested split.
fn covers_paths(
  events: &[(usize, ExternalEvent)],
  parent: &Path,
  holder: &ResourceAccessPoint,
  ends: &BTreeMap<Path, bool>,
  mut fallthrough: bool,
) -> bool {
  if ends.get(parent) == Some(&true) { return true; }
  fallthrough |= ends.contains_key(parent);
  for (_, event) in events.iter().rev() {
    let ExternalEvent::Branch { id, branches, .. } = event else { continue };
    if branches.is_empty() { continue; }
    let local = branches.iter().any(|arm| arm.decl_vars.contains(holder));
    fallthrough = branches.iter().enumerate().all(|(a, arm)| {
      if local && !arm.decl_vars.contains(holder) { return true; }
      let mut path = parent.clone();
      path.push((*id, a));
      covers_paths(&arm.e_data, &path, holder, ends, !arm.diverges && fallthrough)
    });
  }
  fallthrough
}

// This binding was produced by the hidden iterator. Its own end gives
// back access through the borrowed reference even on a back edge where
// the iterator still holds the caller loan.
fn item_end(fact: &Ev, holder: &EndPlace, to: &ResourceTy) -> bool {
  to.is_ref()
    && fact
      .transfer_paths
      .iter()
      .any(|p| p.last().is_some_and(|s| s.place == *holder) && p.iter().any(|s| s.place.name == "<iterator>"))
}

fn connects(path: &[HolderStep], from: &EndPlace, to: &EndPlace) -> bool {
  path
    .iter()
    .position(|s| &s.place == from)
    .is_some_and(|i| path[i + 1..].iter().any(|s| &s.place == to))
}
