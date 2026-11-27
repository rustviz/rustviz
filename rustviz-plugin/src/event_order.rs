//! Associate the visitor's drawing events with the effects in the borrowck
//! body. Compare events on one row and on compatible execution paths.
//! A source span identifies the emitting expression; its byte position is
//! never an evaluation order.
use crate::expr_visitor::ExprVisitor;
use crate::facts::schema::{BodyFacts, DropRoute, Ev, Fact, LoanInfo, Row};
use crate::svg_generator::data::{ExternalEvent as E, ResourceAccessPoint, ResourceTy};
use rustc_middle::mir::{Body, Location, TerminatorKind};
use rustc_span::Span;
use std::collections::{BTreeMap, BTreeSet, HashMap};

// (basic-block RPO index, statement index, effect phase), as in Ev::order.
// Releasing the value in a slot precedes the write at the same location.
type Point = (usize, usize, i8, usize);
const BEFORE_STORE: i8 = -1;

fn normal_facts(row: &Row) -> impl Iterator<Item = &Ev> + Clone {
    row.facts.iter().filter(|e| e.drop_route != DropRoute::Unwind)
}

fn field_store_search_across_rows(row: &Row, straight_line: bool) -> bool {
    straight_line && normal_facts(row).next().is_none()
}

fn fact_point(f: &Ev) -> Point {
    (f.order.0, f.order.1, f.order.2 as i8, 0)
}

fn end(e: &E) -> bool {
    matches!(e, E::StaticDie { .. } | E::MutableDie { .. } | E::RefDie { .. })
}

struct Order<'a, 'tcx> {
    body: &'a Body<'tcx>,
    tcx: rustc_middle::ty::TyCtxt<'tcx>,
    facts: &'a BodyFacts,
    activations: BTreeMap<usize, Point>,
    spans: &'a HashMap<usize, Span>,
    field_overwrite_ends: &'a BTreeSet<usize>,
    ends: &'a BTreeMap<usize, Vec<usize>>,
    issues: &'a BTreeMap<usize, usize>,
    closure_arrivals: &'a BTreeMap<usize, Location>,
    transfers: &'a BTreeMap<usize, crate::facts::value_flow::Site>,
}

fn place(r: &ResourceTy) -> Option<String> {
    match r {
        ResourceTy::Value(v) if v.is_fn() => Some(format!("{}()", v.name())),
        ResourceTy::Value(_) | ResourceTy::Deref(_) | ResourceTy::DerefField(..) | ResourceTy::Index(..) => {
            Some(r.name())
        }
        _ => None,
    }
}

fn names_lender(r: &ResourceTy, l: &LoanInfo) -> bool {
    let place = place(r);
    place.as_deref() == Some(&l.lender)
        || l.through.as_deref().is_some_and(|t| place.as_deref() == Some(t))
        || matches!(r, ResourceTy::Caller) && l.source == "caller"
}

impl Order<'_, '_> {
    fn loan(&self, id: usize) -> Option<&LoanInfo> {
        self.facts.loans.iter().find(|l| l.loan == id)
    }

    fn matches(&self, ev: &E, f: &Ev) -> bool {
        let owner = match (ev, &f.fact) {
            (E::GoOutOfScope { ro, .. } | E::OwnerDropAtReassign { ro, .. }, _) => Some(ro),
            (E::Bind { to, .. } | E::Copy { to, .. } | E::Move { to, .. }, Fact::Init { .. }) => to.extract_rap(),
            (E::Move { from, .. }, Fact::Move { .. }) => from.extract_rap(),
            _ => None,
        };
        if owner.and_then(ResourceAccessPoint::owner_local).zip(f.place_local).is_some_and(|(a, b)| a != b) { return false; }
        match (ev, &f.fact) {
            (
                E::StaticDie { from, to, .. } | E::MutableDie { from, to, .. } | E::RefDie { from, to, .. },
                Fact::BorrowEnd { loan, holder, .. } | Fact::HoldEnd { loan, holder, .. },
            ) => {
                if let Some(ids) = self.ends.get(&ev.get_id()) {
                    ids.contains(loan) && (place(from).as_deref() == Some(holder) || ids.len() == 1)
                } else {
                    self.loan(*loan).is_some_and(|l| {
                        names_lender(to, l)
                            && place(from).is_some_and(|p| {
                                p == *holder || l.direct_holders.contains(&p) || l.via_call.contains(&p)
                            })
                    })
                }
            }
            (
                E::PassByStaticReference { from, to, .. } | E::PassByMutableReference { from, to, .. },
                Fact::Borrow { loan, holder, .. },
            ) => place(to).as_deref() == Some(holder) && self.loan(*loan).is_some_and(|l| names_lender(from, l)),
            (
                E::StaticBorrow { from, to, .. } | E::MutableBorrow { from, to, .. },
                Fact::Borrow { loan, holder, .. },
            ) => self.loan(*loan).is_some_and(|l| {
                names_lender(from, l) && place(to).is_some_and(|p| p == *holder || l.via_call.contains(&p))
            }),
            (E::Move { from, to, .. }, Fact::Move { path, to: dest, .. })
            | (E::Copy { from, to, .. }, Fact::Copy { path, to: dest, .. }) => {
                place(from).as_deref() == Some(path)
                    && (place(to).as_deref() == Some(dest)
                        || matches!(to, ResourceTy::Anonymous) && matches!(dest.as_str(), "<operand>" | "<temp>"))
            }
            (E::Bind { to, .. }, Fact::Init { path, .. }) => place(to).as_deref() == Some(path),
            (E::Move { from, to, .. } | E::Copy { from, to, .. }, Fact::Init { path, .. }) => {
                from.extract_rap().is_none_or(|r| r.is_fn()) && place(to).as_deref() == Some(path)
            }
            (E::GoOutOfScope { ro, .. }, Fact::ScopeEnd { path, .. } | Fact::Drop { path, at_reassign: false, .. })
            | (E::OwnerDropAtReassign { ro, .. }, Fact::Drop { path, at_reassign: true, .. }) => {
                self.storage_of(ro.name()) == path
            }
            _ => false,
        }
    }

    fn point(&self, ev: &E, row: &Row, straight_line: bool) -> Option<Point> {
        if let Some(site) = self.transfers.get(&ev.get_id()).filter(|s| s.row == row.line) {
            let rank = self.body.basic_blocks.reverse_postorder().iter().position(|b| *b == site.at.block)?;
            return Some(site.order(rank));
        }
        if let Some(at) = self.closure_arrivals.get(&ev.get_id()) {
            let rank = self.body.basic_blocks.reverse_postorder().iter().position(|b| *b == at.block)?;
            return Some((rank, at.statement_index, 1, 0));
        }
        if let Some(id) = self.issues.get(&ev.get_id()) {
            return row.facts.iter().find(|f| matches!(f.fact, Fact::Borrow { loan, .. } if loan == *id)).map(fact_point);
        }
        let source = self.spans.get(&ev.get_id());
        let blocks = self.body.basic_blocks.reverse_postorder();
        if let E::Branch { branches, .. } = ev {
            let owner = branches.first()?.source?.branch;
            let tests = self.facts.tests_of(owner);
            let switch = *tests.first().filter(|_| tests.len() == 1)?;
            let rank = blocks.iter().position(|&bb| bb == switch.block)?;
            return Some((rank, switch.statement_index, 0, 0));
        }
        let is_end = end(ev);
        // A whole-value store also replaces each field. The displayed
        // borrow/copy into a field takes effect at the destination, after
        // its RHS has run. The old field stops holding its previous value
        // at that store, even if another copy keeps the loan alive.
        let replacing = self.field_overwrite_ends.contains(&ev.get_id());
        let field = match ev {
            E::StaticBorrow { to, .. } | E::MutableBorrow { to, .. } | E::Move { to, .. } | E::Copy { to, .. } => {
                to.extract_rap()
            }
            _ if replacing => endpoints(ev).0.extract_rap(),
            _ => None,
        };
        if let Some(field) = field.filter(|r| r.is_member() && (r.is_ref() || r.holds_borrow())) {
            let root = self.storage_of(field.name());
            let search_across_rows = field_store_search_across_rows(row, straight_line);
            let mut stores = self
                .facts
                .rows
                .iter()
                .filter(|r| r.line == row.line || search_across_rows)
                .flat_map(normal_facts)
                .filter(|e| {
                    let Fact::Init { path, reassign: true, .. } = &e.fact else {
                        return false;
                    };
                    if path != root {
                        return false;
                    }
                    let loc = e.occurrence.at;
                    let hir = rustc_utils::BodyExt::location_to_hir_id(self.body, loc);
                    let rustc_hir::Node::Expr(expr) = self.tcx.hir_node(hir) else {
                        return false;
                    };
                    source.is_some_and(|s| expr.span.contains(*s) || s.contains(expr.span))
                });
            if let Some(store) = stores.next().filter(|_| stores.next().is_none()) {
                if !replacing {
                    return Some(fact_point(store));
                }
                // A user destructor may still read this field. The normal
                // successor installs the new value only after it returns.
                let drops = normal_facts(row).filter(|e| {
                    matches!(&e.fact,
                    Fact::Drop {path,at_reassign:true,..} if path==root)
                        && self.earlier(fact_point(e), fact_point(store))
                });
                if let Some(drop) = drops.clone().next().filter(|_| drops.count() == 1) {
                    let mut ends = self
                        .facts
                        .rows
                        .iter()
                        .flat_map(normal_facts)
                        .filter(|f| self.matches(ev, f) && matches!(f.fact, Fact::BorrowEnd { .. }));
                    if let Some(first) = ends
                        .next()
                        .filter(|f| self.earlier(fact_point(f), fact_point(drop)) && ends.all(|g| g.order == f.order))
                    {
                        return Some(fact_point(first));
                    }
                }
                return Some((store.order.0, store.order.1, BEFORE_STORE, 0));
            }
        }
        if matches!(ev, E::StaticBorrow { .. } | E::MutableBorrow { .. }) {
            if let Some((store, _)) = self.field_generation(ev, row) {
                return Some(fact_point(store));
            }
        }
        let mut candidates: Vec<_> = normal_facts(row).filter(|e| self.matches(ev, e)).collect();
        if candidates.is_empty() && (is_end || matches!(ev, E::GoOutOfScope { .. })) {
            // Cleanup for a terminal call is displayed at that call. Its
            // drop and release still take place at their actual MIR points.
            let mut cleanup = row.facts.iter().filter(|f| {
                f.drop_route == DropRoute::Unwind && self.matches(ev, f)
                    && matches!(self.body[f.display_location.block].terminator().kind, TerminatorKind::Call { target: None, .. })
            });
            if let Some(first) = cleanup.next() {
                if cleanup.all(|f| f.display_location == first.display_location && f.order == first.order) {
                    return Some(fact_point(first));
                }
            }
        }
        // Closure captures passed to calls have a presentation-only
        // function column. Resolve the actual MIR capture by its expression
        // and its place when that identifies a single borrow.
        if candidates.is_empty() {
            if let E::PassByStaticReference { from, to, .. } | E::PassByMutableReference { from, to, .. } = ev {
                candidates.extend(normal_facts(row).filter(|f| {
                    let Fact::Borrow { loan, .. } = &f.fact else {
                        return false;
                    };
                    let Some(info) = self.loan(*loan) else { return false };
                    let loc = f.occurrence.at;
                    let site = self.body.source_info(loc).span;
                    if source.is_some_and(|span| span.contains(site)) && names_lender(from, info) {
                        return true;
                    }
                    // A macro can borrow through several dereferences of a
                    // named receiver. Look up its operand local in MIR.
                    if info.in_macro.as_ref() != to.extract_rap().map(|r| r.name()) || info.in_macro.is_none() {
                        return false;
                    }
                    let Some(statement) = self.body.basic_blocks[loc.block].statements.get(loc.statement_index) else {
                        return false;
                    };
                    let rustc_middle::mir::StatementKind::Assign(assign) = &statement.kind else {
                        return false;
                    };
                    let rustc_middle::mir::Rvalue::Ref(_, _, borrowed) = assign.1 else {
                        return false;
                    };
                    self.facts.vars.iter().any(|v| {
                        v.local == borrowed.local.as_usize() && Some(v.name.as_str()) == place(from).as_deref()
                    })
                }));
            }
        }
        if is_end {
            if let Some((_, ids)) = self.field_generation(ev, row) {
                // Replacing a slot and releasing its lender need not
                // coincide. This arrow gives the resource back.
                let mut releases = normal_facts(row).filter_map(|f| match &f.fact {
                    Fact::BorrowEnd { loan, .. }
                        if ids.contains(loan) && self.loan(*loan).is_some_and(|l| names_lender(endpoints(ev).1, l)) =>
                    {
                        Some(fact_point(f))
                    }
                    _ => None,
                });
                let first = releases.next()?;
                return releases.all(|p| p == first).then_some(first);
            }
            let (holder, lender) = endpoints(ev);
            // A write through a reference can leave the presentation
            // holder inside the place that the MIR end names. The same
            // MIR edge must also return the reference to that place.
            if let Some(holder) = place(holder) {
                // A slot can stop holding its old value at a store while
                // a copy keeps the old loan live. Repoint identifies both
                // generations even when there is no BorrowEnd here.
                let mut stores = normal_facts(row).filter(|f| {
                    matches!(&f.fact, Fact::Repoint {
                    holder: path, old, new, .. } if *path == holder && old.iter().any(|id|
                        !new.contains(id) && self.ends.get(&ev.get_id()).is_none_or(|ids| ids.contains(id)) && self.loan(*id).is_some_and(|l| names_lender(lender,l))))
                });
                if let Some(store) = stores.next().filter(|_| stores.next().is_none()) {
                    return Some((store.order.0, store.order.1, BEFORE_STORE, 0));
                }
                let via_holder = |f: &Ev| {
                    let Fact::BorrowEnd { loan, holder: via, .. } = &f.fact else {
                        return false;
                    };
                    self.loan(*loan).is_some_and(|l| names_lender(lender, l))
                        && normal_facts(row).any(|other| {
                            other.order == f.order
                                && matches!(&other.fact, Fact::BorrowEnd {
                            lender: root, holder: returned, .. } if root == &holder && returned == via)
                        })
                };
                if candidates.is_empty() {
                    candidates.extend(normal_facts(row).filter(|f| via_holder(f)));
                }
            }
            let gives_back = matches!(ev, E::StaticDie { still_borrowed_by, .. } | E::MutableDie { still_borrowed_by, .. }
                if still_borrowed_by.is_empty());
            let preferred: Vec<_> =
                candidates.iter().copied().filter(|e| matches!(e.fact, Fact::BorrowEnd { .. }) == gives_back).collect();
            if !preferred.is_empty() {
                candidates = preferred;
            }
        }
        if let E::StaticBorrow { to, .. } | E::MutableBorrow { to, .. } = ev {
            // A borrow carried back through a call reaches its destination
            // on return. Borrowing the argument precedes the call.
            if let Some(p) = place(to) {
                let returned: Vec<_> = candidates
                    .iter()
                    .filter_map(|e| match &e.fact {
                        Fact::Borrow { loan, holder, .. }
                            if holder == &p || self.loan(*loan)?.via_call.contains(&p) =>
                        {
                            Some(*loan)
                        }
                        _ => None,
                    })
                    .collect();
                if !returned.is_empty() {
                    let repoints: Vec<_> = normal_facts(row)
                        .filter(|e| {
                            matches!(&e.fact,
                        Fact::Repoint { holder, new, .. } if holder == &p && returned.iter().any(|l| new.contains(l)))
                        })
                        .collect();
                    if !repoints.is_empty() {
                        candidates = repoints;
                    }
                }
            }
        }
        if candidates.is_empty() && is_end {
            // Some earlier ends are drawn at a later overwrite. Use the
            // original edge only if it is unambiguous and precedes a
            // store to the same slot along this path.
            if let Some(holder) = place(endpoints(ev).0) {
                let stores: Vec<_> = normal_facts(row)
                    .filter(|e| match &e.fact {
                        Fact::Init { path, reassign: true, .. } | Fact::Repoint { holder: path, .. } => {
                            *path == holder || path == self.storage_of(&holder)
                        }
                        _ => false,
                    })
                    .collect();
                if !stores.is_empty() {
                    let mut ends = self.facts.rows.iter().flat_map(normal_facts).filter(|f| self.matches(ev, f));
                    if let Some(first) = ends.next().filter(|e| ends.all(|f| f.order == e.order)) {
                        if stores.iter().any(|store| self.earlier(fact_point(first), fact_point(store))) {
                            candidates.push(first);
                        }
                    }
                }
            }
        }
        if candidates.is_empty() {
            if let E::GoOutOfScope { ro, .. } = ev {
                let mut deaths = self.facts.rows.iter().flat_map(normal_facts).filter(|f| matches!(&f.fact,
                    Fact::ScopeEnd { path, .. } | Fact::Drop { path, at_reassign: false, .. } if path == self.storage_of(ro.name())));
                if let Some(death) = deaths.next().map(|f| f.order).filter(|d| deaths.all(|f| f.order == *d)) {
                    return Some((death.0, death.1, death.2 as i8, 0));
                }
                // At a diverging call the walk draws unwind scope exits.
                // MIR has no StorageDead on a normal return path there.
                let mut exits = normal_facts(row).filter_map(|e| {
                    let b = e.order.0;
                    let data = &self.body.basic_blocks[*blocks.get(b)?];
                    matches!(data.terminator().kind, TerminatorKind::Call { target: None, .. }).then_some((
                        b,
                        data.statements.len(),
                        3,
                        0,
                    ))
                });
                let at = exits.next()?;
                return exits.all(|e| e == at).then_some(at);
            }
            return None;
        }
        let at_site: Vec<_> = candidates
            .iter()
            .copied()
            .filter(|e| {
                source.is_some_and(|s| {
                    let mir = self.body.source_info(e.occurrence.at).span;
                    s.contains(mir) || s.source_callsite().contains(mir.source_callsite())
                })
            })
            .collect();
        let sites = if at_site.is_empty() { &candidates } else { &at_site };
        // The single displayed method event includes what the callee does and
        // its return. A two-phase receiver's reservation can precede argument
        // evaluation. Its activation is the call itself.
        if matches!(ev, E::PassByMutableReference { .. }) {
            if let [site] = sites.as_slice() {
                if let Fact::Borrow { loan, .. } = site.fact {
                    if let Some(&call) = self.activations.get(&loan) {
                        let receiver = place(endpoints(ev).0);
                        if normal_facts(row).any(|f| {
                            self.earlier(fact_point(site), fact_point(f))
                                && self.earlier(fact_point(f), call)
                                && match &f.fact {
                                    Fact::Repoint { holder, .. } => Some(holder) == receiver.as_ref(),
                                    Fact::Borrow { loan, .. } => {
                                        self.loan(*loan).is_some_and(|l| names_lender(endpoints(ev).0, l))
                                    }
                                    _ => false,
                                }
                        }) {
                            return Some(call);
                        }
                    }
                }
            }
        }
        let after_binding;
        let sites = if is_end && sites.iter().any(|e| e.order != sites[0].order) {
            // A pattern can fail before its binding ever acquires the loan.
            // The end on that edge is not an end on the binding's column.
            let holder = place(endpoints(ev).0)?;
            let mut starts = normal_facts(row).filter(|e| {
                matches!(&e.fact,
                Fact::Init { path, reassign: false, .. } if *path == holder)
            });
            let init = starts.next()?.order;
            if starts.next().is_some() {
                return None;
            }
            let dominates = |e: &&&Ev| {
                let at = e.order;
                if init.0 == at.0 {
                    return init.1 <= at.1;
                }
                self.body.basic_blocks.dominators().dominates(blocks[init.0], blocks[at.0])
            };
            after_binding = sites.iter().filter(dominates).copied().collect::<Vec<_>>();
            if after_binding.is_empty() {
                return None;
            }
            &after_binding
        } else {
            sites
        };
        let at = sites[0].order;
        sites.iter().all(|e| e.order == at).then_some((at.0, at.1, at.2 as i8, 0))
    }

    /// The fact source of a reborrow can differ from the displayed
    /// receiver (e.g. an iterator hands an element to a reference).
    /// A parent loan given back on this path must outlast its reborrows.
    fn returned_child(&self, a: &E, b: &E) -> bool {
        if !gives_back_to(a, b)
            || !matches!(b, E::StaticDie { still_borrowed_by, .. } | E::MutableDie { still_borrowed_by, .. }
            if still_borrowed_by.is_empty())
        {
            return false;
        }
        let (from, _) = endpoints(a);
        let (parent, owner) = endpoints(b);
        let (Some(child), Some(parent)) = (place(from), place(parent)) else {
            return false;
        };
        let parent_ids = self.ends.get(&b.get_id()).filter(|ids| !ids.is_empty());
        let child_ids = self.ends.get(&a.get_id()).filter(|ids| !ids.is_empty());
        let matches_parent = |l: &LoanInfo| {
            parent_ids.map_or_else(
                || {
                    l.direct_holders.contains(&parent)
                        && (place(owner).as_deref() == Some(&l.lender)
                            || matches!(owner, ResourceTy::Caller) && l.source == "caller")
                },
                |ids| ids.contains(&l.loan),
            )
        };
        let depends = |l: &LoanInfo| {
            (matches_parent(l) && l.holders.contains(&child))
                || l.made_through.iter().any(|id| self.loan(*id).is_some_and(&matches_parent))
        };
        let mut children = self.facts.loans.iter().filter(|l| {
            child_ids.map_or_else(
                || {
                    (l.direct_holders.contains(&child) || l.via_call.contains(&child))
                        && (names_lender(endpoints(a).1, l) || depends(l))
                },
                |ids| ids.contains(&l.loan),
            )
        });
        children.next().is_some_and(depends) && children.all(depends)
    }

    fn writes_reference(&self, e: &Ev) -> bool {
        let (rank, i, _) = e.order;
        let bb = self.body.basic_blocks.reverse_postorder()[rank];
        self.body.basic_blocks[bb].statements.get(i).is_some_and(|stmt| {
            matches!(&stmt.kind,rustc_middle::mir::StatementKind::Assign(assign)
                if assign.0.ty(&self.body.local_decls,self.tcx).ty.is_ref())
        })
    }

    fn storage_of<'a>(&'a self, place: &'a str) -> &'a str {
        self.facts.columns.iter().find(|c| c.name == place).map_or(place, |c| c.root.as_str())
    }

    fn earlier(&self, x: Point, y: Point) -> bool {
        x < y
            && (x.0 == y.0 || {
                let blocks = self.body.basic_blocks.reverse_postorder();
                self.body.basic_blocks.dominators().dominates(blocks[x.0], blocks[y.0])
            })
    }

    /// A field assignment can have two sites on one drawable arrow. The
    /// source is read when building the RHS, the destination changes at
    /// the store. Use the read site against an end on the source column.
    fn on_column(&self, ev: &E, boundary: &E, point: Option<Point>, row: &Row) -> Option<Point> {
        if !end(boundary) {
            return point;
        }
        let (from, to) = match ev {
            E::Copy { from, to, .. }
            | E::Move { from, to, .. }
            | E::StaticBorrow { from, to, .. }
            | E::MutableBorrow { from, to, .. } => (from, to),
            _ => return point,
        };
        let Some(src) = place(from) else { return point };
        if place(endpoints(boundary).0) != Some(src.clone()) || !to.extract_rap().is_some_and(|r| r.is_member()) {
            return point;
        }
        let dst = place(to)?;
        let source = self.spans.get(&ev.get_id())?;
        let mut reads = normal_facts(row).filter(|e| {
            let loc = e.occurrence.at;
            if !source.contains(self.body.source_info(loc).span) {
                return false;
            }
            match &e.fact {
                Fact::Borrow { loan, holder, .. } => {
                    holder == &dst
                        && self
                            .facts
                            .loans
                            .iter()
                            .any(|l| l.loan == *loan && (l.through.as_ref() == Some(&src) || l.lender == src))
                }
                Fact::Copy { path, .. } | Fact::Move { path, .. } => path == &src,
                _ => false,
            }
        });
        let first = reads.next()?;
        reads.all(|f| f.order == first.order).then_some((first.order.0, first.order.1, first.order.2 as i8, 0))
    }

    fn before(&self, a: &E, b: &E, x: Option<Point>, y: Option<Point>, row: &Row) -> bool {
        // Each value and each field must stop holding its loan by
        // its own storage exit, including when a loop closing row
        // summarizes multiple per-iteration MIR exits.
        if let E::GoOutOfScope { ro, .. } = b {
            if end(a)
                && endpoints(a).0.extract_rap() == Some(ro)
                && self.facts.vars.iter().any(|v| v.name == self.storage_of(ro.name()))
            {
                return true;
            }
        }
        if matches!(a,E::GoOutOfScope{ro,..} if end(b) && endpoints(b).0.extract_rap()==Some(ro)) {
            return false;
        }
        let x = self.on_column(a, b, x, row);
        let y = self.on_column(b, a, y, row);
        let (Some(x), Some(y)) = (x, y) else {
            return self.returned_child(a, b) && !self.returned_child(b, a);
        };
        if !end(a) && !end(b) {
            // A presentation-only bind or copy can have both a source
            // and destination effect. Only order ordinary events here
            // when they represent the call activation or a destructor.
            let activation = |e: &E, p| {
                matches!(e, E::PassByMutableReference { .. }) && self.activations.values().any(|&at| at == p)
            };
            let drop = |e: &E| matches!(e, E::OwnerDropAtReassign { .. });
            let install = |e: &E| matches!(e, E::Bind { .. } | E::Copy { .. } | E::Move { .. });
            // Reuse of one visual column does not merge the old local's
            // destructor and the next local's initialization into one effect.
            let sibling = |e: &E, other: &E| {
                let E::GoOutOfScope { ro, .. } = e else { return false };
                columns(other).into_iter().flatten().any(|r| ro.hash() == r.hash()
                    && ro.owner_local().zip(r.owner_local()).is_some_and(|(a, b)| a != b))
            };
            if !(sibling(a, b) || sibling(b, a) || activation(a, x) || activation(b, y) || (drop(a) && install(b)) || (drop(b) && install(a))) {
                return false;
            }
        }
        if !related(a, b) {
            return false;
        }
        if x != y {
            return self.earlier(x, y);
        }
        // At a common end, give a child back to its parent first. A
        // write through that parent instead replaces one of the values
        // reaching the point; it doesn't end the containing reference.
        if !gives_back_to(a, b) || gives_back_to(b, a) {
            return false;
        }
        let parent = place(endpoints(b).0);
        let indirect = endpoints(a).0.extract_rap().map(|r| ResourceTy::Deref(r.clone()).name());
        !normal_facts(row).any(|f| {
            let Fact::Init { path, reassign: true, .. } = &f.fact else {
                return false;
            };
            (f.order.0, f.order.1) == (x.0, x.1)
                && self.writes_reference(f)
                && (parent.as_ref() == Some(path) || indirect.as_ref() == Some(path))
        })
    }

    // A variable declared in an arm has a single column. The renderer
    // projects its arm history and enclosing history onto that column.
    // Preserve proven dependencies across those lists if the two effects
    // actually land on the same dot. Sibling arms never order each other.
    fn across_arms<'e>(&self, events: &'e [(usize, E)], parents: &mut Vec<&'e [(usize, E)]>, layout: &mut RowLayout) {
        for (line, ev) in events {
            let E::Branch { branches, .. } = ev else {
                for parent in parents.iter().flat_map(|p| p.iter()).filter(|(l, e)| {
                    l == line && (end(ev) || end(e)) && related(ev, e) && !matches!(e, E::Branch { .. })
                }) {
                    let Some(row) = self.facts.rows.iter().find(|r| r.line == *line) else {
                        continue;
                    };
                    let other = &parent.1;
                    let (x, y) = (self.point(ev, row, false), self.point(other, row, false));
                    for (a, b, first, second) in [(ev, other, x, y), (other, ev, y, x)] {
                        if self.before(a, b, first, second, row) {
                            layout.projected.insert((a.get_id(), b.get_id()));
                        }
                    }
                }
                continue;
            };
            parents.push(events);
            for arm in branches {
                self.across_arms(&arm.e_data, parents, layout);
            }
            parents.pop();
        }
    }

    // A unique loan end before the test expires the reference on entry to
    // both legs, including when the exit pass has no arm copy to remove.
    fn remove_ends_before_fork(&self, events: &mut [(usize, E)], layout: &mut RowLayout) {
        for i in 0..events.len() {
            let (line, branch) = &events[i];
            let E::Branch { branches, live_vars, .. } = branch else { continue };
            let Some(row) = self.facts.rows.iter().find(|r| r.line == *line) else {
                continue;
            };
            let Some(fork) = self.point(branch, row, false) else {
                continue;
            };
            let mut duplicates = BTreeSet::new();
            for (_, e) in events.iter().filter(|(l, e)| l == line && end(e)) {
                let Some(ids) = self.ends.get(&e.get_id()) else {
                    continue;
                };
                let Some(at) = self.point(e, row, false).filter(|p| self.earlier(*p, fork)) else {
                    continue;
                };
                if ids.is_empty()
                    || !ids.iter().all(|id| {
                        let mut ends =
                            normal_facts(row).filter(|f| matches!(f.fact,Fact::BorrowEnd{loan,..} if loan==*id));
                        ends.next().is_some_and(|f| fact_point(f) == at) && ends.all(|f| fact_point(f) == at)
                    })
                {
                    continue;
                }
                if normal_facts(row).any(|f| {
                    matches!(&f.fact,Fact::Repoint{holder,..} if place(endpoints(e).0).as_ref()==Some(holder))
                        && self.earlier(at, fact_point(f))
                        && self.earlier(fact_point(f), fork)
                }) {
                    continue;
                }
                let mut duplicated = false;
                for (_, other) in branches.iter().flat_map(|arm| &arm.e_data) {
                    if end(other) && endpoints(e) == endpoints(other) && self.point(other, row, false) == Some(at) {
                        duplicates.insert(other.get_id());
                        duplicated = true;
                    }
                }
                // An early return may contain only the scope exit of a
                // reference whose end is already before the fork (#1606).
                // A new assignment in an arm needs its own join description.
                if let Some(holder) = endpoints(e).0.extract_rap() {
                    let exits = branches.iter().any(|arm| arm.diverges
                        && arm.e_data.iter().any(|(_, ev)| ev.is_gos_ev() == Some(holder)));
                    let keeps_end = branches.iter().flat_map(|arm| &arm.e_data).all(|(_, ev)|
                        ev.is_gos_ev() == Some(holder) || !crate::expr_visitor::touches(ev, holder));
                    if duplicated || holder.is_ref() && live_vars.contains(holder) && exits && keeps_end {
                        layout.ended_at_fork.insert((branch.get_id(), *holder.hash()));
                    }
                }
            }
            if let E::Branch { branches, .. } = &mut events[i].1 {
                for arm in branches {
                    arm.e_data.retain(|(_, e)| !duplicates.contains(&e.get_id()));
                    for arrows in arm.line_map.values_mut() {
                        arrows.retain(|e| !duplicates.contains(&e.get_id()));
                    }
                }
            }
        }
    }

    /// Assign a join's displayed end only when every end edge of this loan
    /// on the row belongs to a specific drawn arm. An end at the switch
    /// belongs to its successor, even though its source precedes both arms.
    fn place_arm_ends(
        &self,
        events: &mut Vec<(usize, E)>,
        map: &mut BTreeMap<usize, Vec<E>>,
        next: &mut usize,
        layout: &mut RowLayout,
    ) {
        self.remove_ends_before_fork(events, layout);
        let mut removed = BTreeSet::new();
        for i in 0..events.len() {
            let (line, branch) = &events[i];
            let E::Branch {
                split_point,
                merge_point,
                branches,
                branch_type: crate::svg_generator::data::BranchType::If(..),
                ..
            } = branch
            else {
                continue;
            };
            if split_point != merge_point || branches.len() != 2 {
                continue;
            }
            let Some(row) = self.facts.rows.iter().find(|r| r.line == *line) else {
                continue;
            };
            let Some((rank, at, _, _)) = self.point(branch, row, false) else {
                continue;
            };
            let blocks = self.body.basic_blocks.reverse_postorder();
            let switch = Location { block: blocks[rank], statement_index: at };
            let mut edits = Vec::new();
            for (_, e) in events.iter().filter(|(l, e)| l == line && end(e)) {
                let Some(&[id]) = self.ends.get(&e.get_id()).map(Vec::as_slice) else {
                    continue;
                };
                let Some(info) = self.loan(id) else { continue };
                if info.end_lines != [*line]
                    || info.holders.len() != 1
                    || Some(&info.holders[0]) != place(endpoints(e).0).as_ref()
                {
                    continue;
                }
                if branches
                    .iter()
                    .any(|a| a.e_data.iter().any(|(_, other)| end(other) && endpoints(other) == endpoints(e)))
                {
                    continue;
                }
                let holder = place(endpoints(e).0);
                let lender = place(endpoints(e).1);
                let edges: Vec<_> = normal_facts(row)
                    .filter(|f| match &f.fact {
                        Fact::BorrowEnd { loan, holder: from, lender: to, .. } => {
                            *loan == id && holder.as_ref() == Some(from) && lender.as_ref() == Some(to)
                        }
                        _ => false,
                    })
                    .collect();
                let mut dests = BTreeMap::new();
                for edge in &edges {
                    let entry = edge.location == switch;
                    let source = if entry {
                        self.facts.entered(switch, edge.occurrence.successor)
                    } else {
                        edge.arm.and_then(|i| row.arms.get(i)).and_then(|a| {
                            self.facts.entered(a.edge.switch, Some(a.edge.successor.start_location()))
                        })
                    };
                    let Some(arm) = source.and_then(|s| branches.iter().position(|b| b.source == Some(s))) else { break };
                    if dests.insert(arm, (entry, fact_point(edge))).is_some() {
                        break;
                    }
                }
                if !edges.is_empty()
                    && dests.len() == edges.len()
                    && edges.len()
                        == normal_facts(row).filter(|f| matches!(f.fact, Fact::BorrowEnd{loan,..} if loan==id)).count()
                {
                    edits.push((e.clone(), dests));
                }
            }
            let line = *line;
            let E::Branch { branches, .. } = &mut events[i].1 else { unreachable!() };
            for (event, dests) in edits {
                removed.insert(event.get_id());
                let listed = map.get(&line).is_some_and(|es| es.iter().any(|e| e.get_id() == event.get_id()));
                for (arm, (entry, point)) in dests {
                    let mut ev = event.clone();
                    let (E::MutableDie { id, .. } | E::StaticDie { id, .. } | E::RefDie { id, .. }) = &mut ev else {
                        unreachable!()
                    };
                    *id = *next;
                    *next += 1;
                    layout.arm_points.insert(*id, point);
                    let arm = &mut branches[arm];
                    if listed {
                        let arrows = arm.line_map.entry(line).or_default();
                        if entry {
                            arrows.insert(0, ev.clone())
                        } else {
                            arrows.push(ev.clone())
                        }
                    }
                    if entry {
                        arm.e_data.insert(0, (line, ev));
                    } else {
                        arm.e_data.push((line, ev));
                    }
                }
            }
        }
        events.retain(|(_, e)| !removed.contains(&e.get_id()));
        for arrows in map.values_mut() {
            arrows.retain(|e| !removed.contains(&e.get_id()));
        }
        for (_, e) in events {
            if let E::Branch { branches, .. } = e {
                for arm in branches {
                    self.place_arm_ends(&mut arm.e_data, &mut arm.line_map, next, layout);
                }
            }
        }
    }

    /// The loan set written by this store (or replaced, for an old end).
    /// Only distinguish successive stores. Several incoming arrows for
    /// the same call result describe one reference with possible lenders.
    fn field_generation<'a>(&'a self, ev: &E, row: &'a Row) -> Option<(&'a Ev, &'a [usize])> {
        let (from, to) = endpoints(ev);
        let field = (if end(ev) { from } else { to }).extract_rap().filter(|r| r.is_member() && r.is_ref())?;
        let mut stores = normal_facts(row)
            .filter_map(|f| match &f.fact {
                Fact::Repoint { holder, old, new, .. } if holder == field.name() => Some((f, old, new)),
                _ => None,
            })
            .filter(|(f, _, _)| {
                normal_facts(row).any(|g| {
                    matches!(&g.fact, Fact::Repoint{holder,..} if holder==field.name())
                        && f.arm == g.arm
                        && (self.earlier(fact_point(f), fact_point(g)) || self.earlier(fact_point(g), fact_point(f)))
                })
            })
            .filter_map(|(f, old, new)| {
                if let Some(ids) = self.ends.get(&ev.get_id()).filter(|_| end(ev)) {
                    if ids.is_empty() { return None; }
                    // Borrowck can release several generations of the same
                    // lender together. The normal end at the final store
                    // belongs to the value installed there.
                    let released_at_store = new.iter().any(|id| ids.contains(id)) && ids.iter().all(|id|
                        row.facts.iter().any(|e| e.location == f.location && matches!(e.fact,
                            Fact::BorrowEnd { loan, .. } if loan == *id)));
                    if ids.iter().all(|id| new.contains(id)) || released_at_store { return Some((f, new.as_slice())); }
                    // The first store can replace a reference made on an
                    // earlier row (including a call with several inputs).
                    // Later stores' incoming values already have a generation
                    // here, at the store which produced them.
                    let Fact::Repoint { reaching: Some(incoming), .. } = &f.fact else { return None };
                    let issued_here = row.facts.iter().any(|other| matches!(&other.fact,
                        Fact::Repoint { holder, new, .. } if holder == field.name() && ids.iter().any(|id| new.contains(id))));
                    return (!issued_here && ids.iter().all(|id| incoming.contains(id))).then_some((f, incoming.as_slice()));
                }
                let hir = rustc_utils::BodyExt::location_to_hir_id(self.body, f.occurrence.at);
                let rustc_hir::Node::Expr(expr) = self.tcx.hir_node(hir) else {
                    return None;
                };
                if !self.spans.get(&ev.get_id()).is_some_and(|s| expr.span.contains(*s) || s.contains(expr.span)) {
                    return None;
                }
                let ids = if end(ev) { old } else { new };
                let lender = if end(ev) { to } else { from };
                ids.iter()
                    .any(|id| self.loan(*id).is_some_and(|l| names_lender(lender, l)))
                    .then_some((f, ids.as_slice()))
            });
        let generation = stores.next()?;
        stores.next().is_none().then_some(generation)
    }

    fn distinguish_field_writes(&self, ev: &E, row: &Row, layout: &mut RowLayout) {
        if let Some((store, ids)) = self.field_generation(ev, row) {
            let Fact::Repoint { holder, new, .. } = &store.fact else { unreachable!() };
            let slot = FieldGeneration { block: store.order.0, statement: store.order.1, replaced: new != ids };
            layout.field_generations.insert(ev.get_id(), slot);
            let effect = if end(ev) { self.point(ev, row, false) } else { Some(fact_point(store)) };
            // Release arrows go below the first store only when the loan
            // actually lasts beyond it. Merely overwriting a slot cannot
            // tell us that its lender is available again.
            if effect.is_some_and(|at| {
                normal_facts(row).any(|f| {
                    matches!(&f.fact, Fact::Repoint{holder: other,..} if other == holder)
                        && f.arm == store.arm
                        && self.earlier(fact_point(f), at)
                })
            }) {
                layout.later_field_effects.insert(ev.get_id());
            }
            return;
        }
        let Some(field) = endpoints(ev).0.extract_rap().filter(|r| end(ev) && r.is_member() && r.is_ref()) else {
            return;
        };
        // Both generations can remain live for borrowck and end at the
        // same later read. Preserve their individual lender names there.
        let ended: BTreeSet<_> = normal_facts(row)
            .filter_map(|f| match &f.fact {
                Fact::BorrowEnd { loan, .. }
                    if self.loan(*loan).is_some_and(|l| l.direct_holders.contains(field.name())) =>
                {
                    Some(*loan)
                }
                _ => None,
            })
            .collect();
        if ended.len() > 1
            && self.facts.rows.iter().flat_map(normal_facts).any(|e| {
                matches!(&e.fact,Fact::Repoint {holder,old,new,..} if holder == field.name()
                && old.iter().any(|id| ended.contains(id) && !new.contains(id))
                && new.iter().any(|id| ended.contains(id) && !old.contains(id)))
            })
        {
            layout.name_field_ends.insert(ev.get_id());
        }
    }

    fn annotate(&self, ev: &mut E, row: &Row, layout: &mut RowLayout) {
        if let E::MutableBorrow { from, to, is_partial, is_reborrow, break_alt, via, id, .. } = ev {
            if from.extract_rap().is_some_and(|r| r.is_ref() && !r.is_mutref()) {
                let candidates: Vec<_> = normal_facts(row)
                    .filter_map(|f| match &f.fact {
                        Fact::Borrow { lender, loan, .. } if Some(lender) == place(from).as_ref() => {
                            self.loan(*loan).filter(|l| place(to).is_some_and(|p| l.via_call.contains(&p)))
                        }
                        _ => None,
                    })
                    .collect();
                if !candidates.is_empty() && candidates.iter().all(|l| l.kind == "shared") {
                    *ev = E::StaticBorrow {
                        from: from.clone(),
                        to: to.clone(),
                        is_partial: *is_partial,
                        is_reborrow: *is_reborrow,
                        break_alt: break_alt.clone(),
                        via: via.clone(),
                        id: *id,
                        of_ref_var: true,
                    };
                }
            }
        }
        if let E::MutableDie { slot, id, from, to, static_lender, still_borrowed_by, of_ref_var, exhaustion } = ev {
            // A call may keep a shared input beside a mutable input. Use
            // the returned loan's kind for the particular input slot.
            if (*of_ref_var || to.extract_rap().is_some_and(|r| r.is_ref() && !r.is_mutref()))
                && self.ends.get(id).is_some_and(|ids| {
                    !ids.is_empty()
                        && ids
                            .iter()
                            .all(|&id| self.loan(id).is_some_and(|l| l.kind == "shared" && names_lender(to, l)))
                })
            {
                *ev = E::StaticDie {
                    slot: *slot,
                    id: *id,
                    from: from.clone(),
                    to: to.clone(),
                    static_lender: static_lender.clone(),
                    still_borrowed_by: still_borrowed_by.clone(),
                    of_ref_var: true,
                    exhaustion: exhaustion.clone(),
                };
            }
        }
        if let E::RefDie { from, to, id, .. } = ev {
            let holds: Vec<_> = normal_facts(row)
                .filter_map(|f| match &f.fact {
                    Fact::HoldEnd { loan, holder, .. } if Some(holder) == place(from).as_ref() => {
                        self.loan(*loan).filter(|l| names_lender(to, l)).map(|l| (f, l))
                    }
                    _ => None,
                })
                .collect();
            let copied = holds.iter().any(|(hold, _)| {
                normal_facts(row).any(|f| {
                    hold.order.0 == f.order.0
                        && hold.order.1 == f.order.1
                        && matches!(&f.fact, Fact::Copy { path, .. } if Some(path) == place(from).as_ref())
                })
            });
            let repointed = normal_facts(row)
                .any(|f| matches!(&f.fact, Fact::Repoint { holder, .. } if Some(holder) == place(from).as_ref()));
            if copied
                && repointed
                && holds.iter().all(|(_, loan)| loan.kind == "mut")
                && from.extract_rap().is_some_and(|r| r.is_ref() && !r.is_mutref())
            {
                layout.copied_ends.insert(*id);
            }
        }
        let call = self.point(ev, row, false);
        if let (Some(at), E::PassByStaticReference { from, id, .. }) = (call, &*ev) {
            let mut candidates: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            for loan in &self.facts.loans {
                if names_lender(from, loan) {
                    for holder in &loan.direct_holders {
                        candidates.entry(holder).or_default().push(loan.loan);
                    }
                }
            }
            for (holder, ids) in candidates {
                if ids.iter().all(|id| {
                    let mut ends = self
                        .facts
                        .rows
                        .iter()
                        .flat_map(normal_facts)
                        .filter(|f| matches!(f.fact,Fact::BorrowEnd{loan,..} if loan==*id));
                    ends.next().is_some_and(|f| {
                        normal_facts(row).any(|other| other == f) && self.earlier(fact_point(f), at) && ends.next().is_none()
                    })
                }) {
                    layout.ended_before_call.entry(*id).or_default().insert(holder.to_owned());
                }
            }
        }
        if let (Some(call_at), E::PassByMutableReference { id, from, .. }) = (call, ev) {
            if let Some((loan, at)) = normal_facts(row).find_map(|f| match f.fact {
                Fact::Borrow { loan, .. } if self.activations.get(&loan).copied().is_some_and(|p| Some(p) == call) => {
                    Some((loan, fact_point(f)))
                }
                _ => None,
            }) {
                if self.loan(loan).is_some_and(|l| l.through == place(from))
                    && normal_facts(row).any(|f| {
                        matches!(&f.fact,Fact::Repoint{holder,..} if Some(holder)==place(from).as_ref())
                            && self.earlier(at, fact_point(f))
                            && self.earlier(fact_point(f), call_at)
                    })
                {
                    layout.reassigned_receivers.insert(*id);
                }
            }
        }
    }

    fn destructor_before_release(
        &self,
        drop: &E,
        end: &E,
        drop_at: Option<Point>,
        end_at: Option<Point>,
    ) -> Option<String> {
        let E::OwnerDropAtReassign { ro, .. } = drop else { return None };
        let field = endpoints(end).0.extract_rap()?;
        let (drop_at, end_at) = (drop_at?, end_at?);
        if !self.field_overwrite_ends.contains(&end.get_id())
            || self.storage_of(field.name()) != ro.name()
            || end_at.2 != BEFORE_STORE
            || !self.earlier(drop_at, end_at)
        {
            return None;
        }
        let block = self.body.basic_blocks.reverse_postorder()[drop_at.0];
        let TerminatorKind::Drop { place, .. } = &self.body.basic_blocks[block].terminator().kind else {
            return None;
        };
        let adt = place.ty(&self.body.local_decls, self.tcx).ty.ty_adt_def()?;
        adt.has_dtor(self.tcx).then(|| ro.name().clone())
    }

    /// Both arrows must end on the same MIR edge, where the reborrow
    /// through this parent is released and no competing loan survives.
    fn restores_before_end(&self, child: &E, parent: &E, row: &Row, child_at: Option<Point>, parent_at: Option<Point>) -> bool {
        let (holder, through) = endpoints(child);
        let Some(r) = through.extract_rap().filter(|r| r.is_mutref()) else { return false };
        let Some(h) = place(holder) else { return false };
        let mut restored = BTreeSet::new();
        let mut child_points = BTreeSet::new();
        let mut ends = BTreeSet::new();
        for e in &row.facts {
            let Fact::BorrowEnd { loan, .. } = e.fact else { continue };
            let (Some(l), Some(succ)) = (self.loan(loan), e.occurrence.successor) else { continue };
            let at = fact_point(e);
            if child_at.is_none_or(|p| p == at) && l.through.as_ref() == Some(r.name()) && names_lender(through, l)
                && (l.direct_holders.contains(&h) || l.via_call.contains(&h) || l.holders.contains(&h))
                && self.ends.get(&child.get_id()).is_none_or(|ids| ids.contains(&loan)) {
                child_points.insert(at);
                if self.facts.restored_reborrows.contains(&(e.location, succ, loan)) {
                    restored.extend(l.made_through.iter().map(|original| (e.location, succ, *original)));
                }
            }
            if parent_at.is_none_or(|p| p == at) && l.direct_holders.contains(r.name())
                && names_lender(endpoints(parent).1, l)
                && self.ends.get(&parent.get_id()).is_none_or(|ids| ids.contains(&loan)) {
                ends.insert((e.location, succ, loan));
            }
        }
        // An event without an assigned point or loan cannot select one
        // of several distinct releases of the same displayed place.
        (child_at.is_some() || self.ends.contains_key(&child.get_id()) || child_points.len() == 1)
            && !ends.is_empty() && ends.is_subset(&restored)
    }

    fn shadowed_borrow_lender(&self, ev: &E) -> Option<String> {
        let (E::MutableBorrow { from, to, via: None, .. } | E::StaticBorrow { from, to, via: None, .. }) = ev else {
            return None;
        };
        let span = self.spans.get(&ev.get_id())?;
        let holder = place(to)?;
        let mut loans = self.facts.loans.iter().filter(|loan| loan.runs
            && loan.direct_holders.contains(&holder) && names_lender(from, loan)
            && loan.issue.is_some_and(|at| span.contains(self.body.source_info(at).span))
            && self.facts.columns.iter().any(|c| c.name == loan.lender));
        let loan = loans.next()?;
        if loans.next().is_some() { return None; }
        let at = loan.issue?;
        let stmt = self.body.basic_blocks[at.block].statements.get(at.statement_index)?;
        let rustc_middle::mir::StatementKind::Assign(assignment) = &stmt.kind else { return None };
        let rustc_middle::mir::Rvalue::Ref(_, _, borrowed) = &assignment.1 else { return None };
        // The display distinguishes shadowed bindings by giving them a
        // different label from the MIR debug name for that same local.
        let shadowed = self.facts.bindings.locals.get(&borrowed.local).is_some_and(|label| {
            self.body.var_debug_info.iter().any(|var| matches!(var.value,
                rustc_middle::mir::VarDebugInfoContents::Place(p) if p.as_local() == Some(borrowed.local))
                && var.name.as_str() != label)
        });
        shadowed.then(|| loan.lender.clone())
    }

    fn order_list(&self, events: &mut [(usize, E)], map: &mut BTreeMap<usize, Vec<E>>, layout: &mut RowLayout) {
        let mut lines: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, (line, ev)) in events.iter_mut().enumerate() {
            if let E::Branch { branches, .. } = ev {
                for arm in branches {
                    self.order_list(&mut arm.e_data, &mut arm.line_map, layout);
                }
            }
            if let Some(lender) = self.shadowed_borrow_lender(ev) {
                layout.shadowed_borrow_lenders.insert(ev.get_id(), lender);
            }
            lines.entry(*line).or_default().push(i);
        }
        for (line, indices) in lines {
            if indices.len() < 2 {
                continue;
            }
            let Some(row) = self.facts.rows.iter().find(|r| r.line == line) else {
                continue;
            };
            let has_boundary =
                indices.iter().any(|&i| end(&events[i].1) || matches!(events[i].1, E::OwnerDropAtReassign { .. })
                    || matches!(&events[i].1, E::GoOutOfScope { ro, .. } if self.facts.bindings.columns.get(ro.name()).is_some_and(|ls| ls.len() > 1)));
            let exact = |e: &E| self.transfers.get(&e.get_id()).is_some_and(|site| site.row == line);
            let has_repoint = normal_facts(row).any(|f| matches!(f.fact, Fact::Repoint { .. }));
            let has_activation = normal_facts(row).any(|f| matches!(f.fact, Fact::Borrow{loan,..} if self.activations.contains_key(&loan)));
            let ordinary_transfers = !has_boundary && !has_repoint && !has_activation
                && indices.iter().filter(|&&i| exact(&events[i].1)).take(2).count() == 2;
            if !has_boundary && !ordinary_transfers && !has_repoint && !has_activation
            {
                continue;
            }
            for &i in &indices {
                self.annotate(&mut events[i].1, row, layout);
                if let Some(arrow) =
                    map.get_mut(&line).and_then(|m| m.iter_mut().find(|e| e.get_id() == events[i].1.get_id()))
                {
                    if std::mem::discriminant(arrow) != std::mem::discriminant(&events[i].1) {
                        *arrow = events[i].1.clone();
                    }
                }
            }
            // The walk's ordinary events already have evaluation order.
            // Insert each borrow boundary relative to the MIR effects on
            // its columns. An unrelated column or an unlocated event keeps
            // its position unless a proven dependency requires moving it.
            let straight_line = !indices.iter().any(|&i| matches!(events[i].1, E::Branch { .. }));
            let points: Vec<_> = indices
                .iter()
                .map(|&i| {
                    layout
                        .arm_points
                        .get(&events[i].1.get_id())
                        .copied()
                        .or_else(|| self.point(&events[i].1, row, straight_line))
                })
                .collect();
            let mut dependencies = BTreeSet::new();
            for (i, &idx) in indices.iter().enumerate() {
                let a = &events[idx].1;
                self.distinguish_field_writes(a, row, layout);
                for (j, &other) in indices.iter().enumerate().filter(|(j, _)| *j != i) {
                    let b = &events[other].1;
                    let ids = (a.get_id(), b.get_id());
                    if let Some(owner) = self.destructor_before_release(a, b, points[i], points[j]) {
                        layout.after_destructor.insert(ids.1, owner);
                    }
                    if points[i].zip(points[j]).is_some_and(|(x, y)| x.2 >= 0 && y.2 >= 0 && self.earlier(x, y)) {
                        layout.before.insert(ids);
                    }
                    if gives_back_to(a, b) && self.restores_before_end(a, b, row, points[i], points[j]) {
                        layout.restored.insert(ids);
                    }
                    if end(a) && end(b) && points[i].is_some() && points[i] == points[j] && gives_back_to(a, b) {
                        layout.same_dot.entry(ids.1).or_default().push(ids.0);
                    }
                    let (from, to) = endpoints(a);
                    let (later_from, later_to) = endpoints(b);
                    let caller = matches!(to, ResourceTy::Caller) && matches!(later_to, ResourceTy::Caller);
                    let call_source = |source: &ResourceTy| source.extract_rap().is_some_and(|r| r.is_fn());
                    let same_local = match (to, later_from) {
                        (ResourceTy::Value(to), ResourceTy::Value(from)) =>
                            to.is_copy_owner() && to.owner_local().is_some() && to.owner_local() == from.owner_local(),
                        _ => false,
                    };
                    let exact_order = ordinary_transfers && exact(a) && exact(b) && (related(a, b) || caller)
                        && points[i].zip(points[j]).is_some_and(|(x, y)| self.earlier(x, y));
                    if exact_order {
                        layout.transfer_order.insert(ids);
                        if (caller && (call_source(from) || call_source(later_from)))
                            || (same_local && call_source(from) && call_source(later_to))
                        {
                            layout.transfer_stack.extend([ids.0, ids.1]);
                        }
                    }
                    if exact_order || self.before(a, b, points[i], points[j], row) {
                        dependencies.insert(ids);
                        layout.dependencies.insert(ids);
                        if end(a) && end(b) {
                            layout.end_order.insert(ids);
                        }
                    }
                }
            }
            let ordered = dependency_order(indices.len(), |i, j| {
                dependencies.contains(&(events[indices[i]].1.get_id(), events[indices[j]].1.get_id()))
            });
            if let Some(arrows) = map.get_mut(&line) {
                let order = dependency_order(arrows.len(), |i, j| {
                    !(end(&arrows[i]) && end(&arrows[j]))
                        && dependencies.contains(&(arrows[i].get_id(), arrows[j].get_id()))
                });
                if order.iter().copied().ne(0..order.len()) {
                    *arrows = order.iter().map(|&i| arrows[i].clone()).collect();
                }
            }
            if ordered.iter().copied().eq(0..ordered.len()) {
                continue;
            }
            let values: Vec<_> = ordered.iter().map(|&k| events[indices[k]].1.clone()).collect();
            for (&i, ev) in indices.iter().zip(values) {
                events[i].1 = ev;
            }
        }
    }
}

pub(crate) fn order_body(visitor: &mut ExprVisitor<'_, '_>, facts: &BodyFacts) {
    let layout = &mut visitor.row_layout;
    let stored: BTreeSet<_> = layout.stored_lender_loans.values().flatten().copied().collect();
    layout.projected_loan_places.retain(|_, (loan, _)| stored.contains(loan));
    let order = Order {
        activations: visitor
            .bwf
            .borrow_set
            .location_map()
            .values()
            .enumerate()
            .filter_map(|(index, data)| {
                let rustc_borrowck::consumers::TwoPhaseActivation::ActivatedAt(at) = data.activation_location() else {
                    return None;
                };
                let rank = visitor.mir_body.basic_blocks.reverse_postorder().iter().position(|b| *b == at.block)?;
                Some((index, (rank, at.statement_index, 0, 0)))
            })
            .collect(),
        tcx: visitor.tcx,
        body: visitor.mir_body,
        facts,
        spans: &visitor.event_spans,
        transfers: &visitor.transfer_sites,
        field_overwrite_ends: &visitor.field_overwrite_ends,
        ends: &visitor.end_loans,
        issues: &visitor.aggregate_issues,
        closure_arrivals: &visitor.closure_result_arrivals,
    };
    order.place_arm_ends(
        visitor.preprocessed_events,
        visitor.event_line_map,
        visitor.unique_id,
        &mut visitor.row_layout,
    );
    order.across_arms(visitor.preprocessed_events, &mut Vec::new(), &mut visitor.row_layout);
    order.order_list(visitor.preprocessed_events, visitor.event_line_map, &mut visitor.row_layout);
}

/// Read the independent columns in their existing order, emitting any
/// predecessor before its dependent. All dependencies are strict MIR (or
/// replay) order, so this graph is acyclic.
fn dependency_order(n: usize, before: impl Fn(usize, usize) -> bool) -> Vec<usize> {
    fn visit(i: usize, seen: &mut [bool], out: &mut Vec<usize>, before: &impl Fn(usize, usize) -> bool) {
        if seen[i] {
            return;
        }
        seen[i] = true;
        for j in 0..seen.len() {
            if !seen[j] && before(j, i) {
                visit(j, seen, out, before);
            }
        }
        out.push(i);
    }
    let mut seen = vec![false; n];
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        visit(i, &mut seen, &mut out, &before);
    }
    out
}

fn endpoints(e: &E) -> (&ResourceTy, &ResourceTy) {
    match e {
        E::RefDie { from, to, .. } => (from, to),
        _ => crate::svg_generator::data::ResourceAccessPoint_extract(e),
    }
}

fn gives_back_to(a: &E, b: &E) -> bool {
    matches!(a, E::StaticDie { .. } | E::MutableDie { .. })
        && end(b)
        && endpoints(a).1.extract_rap().is_some_and(|r| Some(r) == endpoints(b).0.extract_rap())
}

fn columns(e: &E) -> [Option<&crate::svg_generator::data::ResourceAccessPoint>; 2] {
    match e {
        E::GoOutOfScope { ro, .. } | E::OwnerDropAtReassign { ro, .. } => [Some(ro), None],
        _ => {
            let (from, to) = endpoints(e);
            [from.extract_rap(), to.extract_rap()]
        }
    }
}

fn related(a: &E, b: &E) -> bool {
    if matches!(a, E::Branch { .. }) || matches!(b, E::Branch { .. }) {
        return true;
    }
    let other = columns(b);
    columns(a).into_iter().flatten().any(|p| other.contains(&Some(p)))
}

/// One side of a field store in its borrowck body. Several call-input
/// loans can describe the same reference reaching this store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FieldGeneration {
    block: usize,
    statement: usize,
    pub(crate) replaced: bool,
}

/// What the receiver holds when an input and the loan it carries have different access rights.
#[derive(Debug, Clone, Copy)]
pub(crate) enum StoredLoan {
    SharedInput { mutable: bool, binding: bool },
    Carrier { mutable: bool, binding: bool },
    UniqueCapture,
}

/// Coincident effects on one column can include a dot without an arrow.
#[derive(Default, Debug)]
pub struct RowLayout {
    /// Both column dots and the connecting arrow describe the same aggregate operand.
    pub(crate) pattern_payloads: BTreeMap<usize, (String, &'static str)>,
    pub(crate) aggregate_titles: BTreeMap<usize, String>,
    pub(crate) shadowed_borrow_lenders: BTreeMap<usize, String>,
    pub(crate) returned_call_titles: BTreeMap<usize, String>,
    pub(crate) argument_results: BTreeSet<usize>,
    pub(crate) transfer_stack: BTreeSet<usize>,
    transfer_order: BTreeSet<(usize, usize)>,
    pub(crate) closure_result_holders: BTreeSet<u64>,
    pub(crate) stored_lender_loans: BTreeMap<u64, BTreeSet<usize>>,
    pub(crate) stored_loans: BTreeMap<usize, StoredLoan>,
    /// Borrow events drawn on an ancestor column, with the actual MIR loan and place.
    pub(crate) projected_loan_places: BTreeMap<usize, (usize, crate::facts::ProjectedPlace)>,
    pub(crate) stored_field_ends: BTreeSet<usize>,
    pub(crate) uncertain_projected_stores: BTreeSet<(u64, usize)>,
    pub(crate) shared_projected_stores: BTreeSet<(u64, usize)>,
    pub(crate) shared_projected_calls: BTreeSet<usize>,
    pub(crate) restored: BTreeSet<(usize, usize)>,
    pub(crate) ended_before_call: BTreeMap<usize, BTreeSet<String>>,
    pub(crate) ended_at_fork: BTreeSet<(usize, u64)>,
    pub(crate) after_destructor: BTreeMap<usize, String>,
    pub(crate) dropped_field_loans: BTreeMap<usize, DroppedFieldLoan>,
    pub(crate) short_circuit_forks: BTreeSet<usize>,
    pub(crate) copied_ends: BTreeSet<usize>,
    /// Write event to the proven reference slot and its referent (`**s += 1`).
    pub(crate) retained_references: BTreeMap<usize, (String, String)>,
    pub(crate) reassigned_receivers: BTreeSet<usize>,
    arm_points: BTreeMap<usize, Point>,
    pub(crate) closure_captures: BTreeMap<usize, Vec<usize>>,
    pub(crate) field_generations: BTreeMap<usize, FieldGeneration>,
    pub(crate) later_field_effects: BTreeSet<usize>,
    name_field_ends: BTreeSet<usize>,
    projected: BTreeSet<(usize, usize)>,
    dependencies: BTreeSet<(usize, usize)>,
    end_order: BTreeSet<(usize, usize)>,
    before: BTreeSet<(usize, usize)>,
    same_dot: BTreeMap<usize, Vec<usize>>,
}

#[derive(Debug)]
pub(crate) struct DroppedFieldLoan {
    pub(crate) field: String,
    pub(crate) holder: String,
    /// Whether this is a replacement drop, and whether it is conditional.
    pub(crate) replacement: Option<bool>,
}

impl RowLayout {
    pub fn append(&mut self, other: &mut Self) {
        self.arm_points.append(&mut other.arm_points);
        self.pattern_payloads.append(&mut other.pattern_payloads);
        self.aggregate_titles.append(&mut other.aggregate_titles);
        self.shadowed_borrow_lenders.append(&mut other.shadowed_borrow_lenders);
        self.returned_call_titles.append(&mut other.returned_call_titles);
        self.argument_results.append(&mut other.argument_results);
        self.transfer_stack.append(&mut other.transfer_stack);
        self.transfer_order.append(&mut other.transfer_order);
        self.closure_result_holders.append(&mut other.closure_result_holders);
        self.stored_lender_loans.append(&mut other.stored_lender_loans);
        self.restored.append(&mut other.restored);
        self.stored_loans.append(&mut other.stored_loans);
        self.projected_loan_places.append(&mut other.projected_loan_places);
        self.stored_field_ends.append(&mut other.stored_field_ends);
        self.uncertain_projected_stores.append(&mut other.uncertain_projected_stores);
        self.shared_projected_stores.append(&mut other.shared_projected_stores);
        self.shared_projected_calls.append(&mut other.shared_projected_calls);
        self.ended_at_fork.append(&mut other.ended_at_fork);
        self.after_destructor.append(&mut other.after_destructor);
        self.dropped_field_loans.append(&mut other.dropped_field_loans);
        self.short_circuit_forks.append(&mut other.short_circuit_forks);
        self.copied_ends.append(&mut other.copied_ends);
        self.retained_references.append(&mut other.retained_references);
        self.reassigned_receivers.append(&mut other.reassigned_receivers);
        self.closure_captures.append(&mut other.closure_captures);
        self.field_generations.append(&mut other.field_generations);
        self.later_field_effects.append(&mut other.later_field_effects);
        self.name_field_ends.append(&mut other.name_field_ends);
        self.same_dot.append(&mut other.same_dot);
        self.before.append(&mut other.before);
        self.dependencies.append(&mut other.dependencies);
        self.end_order.append(&mut other.end_order);
        self.projected.append(&mut other.projected);
    }

    pub(crate) fn record_arm_point(&mut self, id: usize, edge: &Ev) {
        self.arm_points.insert(id, fact_point(edge));
    }

    pub(crate) fn has_arm_point(&self, id: usize) -> bool {
        self.arm_points.contains_key(&id)
    }

    pub(crate) fn name_lender(&self, id: usize) -> bool {
        self.field_generations.contains_key(&id) || self.name_field_ends.contains(&id) || self.stored_field_ends.contains(&id)
    }

    pub(crate) fn project(&self, history: &mut [(usize, crate::svg_generator::data::Event)]) {
        if self.projected.is_empty() {
            return;
        }
        let mut by_row: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, (row, _)) in history.iter().enumerate() {
            by_row.entry(*row).or_default().push(i);
        }
        for indices in by_row.values().filter(|indices| indices.len() > 1) {
            let ids: Vec<_> = indices.iter().map(|&i| history[i].1.get_id()).collect();
            let order = dependency_order(indices.len(), |i, j| self.projected.contains(&(ids[i], ids[j])));
            if order.iter().copied().ne(0..order.len()) {
                let values: Vec<_> = order.iter().map(|&i| history[indices[i]].1.clone()).collect();
                for (&i, event) in indices.iter().zip(values) {
                    history[i].1 = event;
                }
            }
        }
    }

    pub(crate) fn order_transfers(&self, events: &mut Vec<E>) {
        if self.transfer_order.is_empty() {
            return;
        }
        let order = dependency_order(events.len(), |i, j|
            self.transfer_order.contains(&(events[i].get_id(), events[j].get_id())));
        if order.iter().copied().ne(0..order.len()) {
            *events = order.iter().map(|&i| events[i].clone()).collect();
        }
    }

    pub(crate) fn precedes(&self, first: usize, second: usize) -> bool {
        self.before.contains(&(first, second))
    }

    pub(crate) fn between(&self, first: usize, middle: usize, last: usize) -> bool {
        self.before.contains(&(first, middle)) && self.before.contains(&(middle, last))
    }

    pub(crate) fn align(&self, placed: &mut [(usize, E)], map: &BTreeMap<usize, Vec<E>>) {
        let anchors: HashMap<_, _> =
            map.iter().flat_map(|(&row, es)| es.iter().map(move |e| (e.get_id(), row))).collect();
        for (row, e) in placed {
            if anchors.contains_key(&e.get_id()) {
                continue;
            }
            if let Some(other) = self.same_dot.get(&e.get_id()) {
                let mut rows = other.iter().filter_map(|id| anchors.get(id));
                if let Some(anchor) = rows.next().filter(|a| rows.all(|r| r == *a)) {
                    *row = *anchor;
                }
            }
        }
    }
}

/// Stacking decides geometry before the renderer expands an event into
/// column histories. Preserve the layout of unrelated arrows and use the
/// semantic sequence for a borrow and its uses on the same dot.
pub(crate) fn replay(placed: &mut [(usize, E)], sequence: &HashMap<usize, usize>, layout: &RowLayout) {
    let mut rows: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, (row, _)) in placed.iter().enumerate() {
        rows.entry(*row).or_default().push(i);
    }
    for indices in rows.values() {
        let ordinary = |e: &E| !end(e) && !matches!(e, E::StaticBorrow { .. } | E::MutableBorrow { .. });
        let order = dependency_order(indices.len(), |i, j| {
            let (a, b) = (&placed[indices[i]].1, &placed[indices[j]].1);
            (layout.dependencies.contains(&(a.get_id(), b.get_id())) || !(ordinary(a) && ordinary(b)))
                && related(a, b)
                && (!(end(a) && end(b)) || layout.end_order.contains(&(a.get_id(), b.get_id())))
                && sequence.get(&a.get_id()).zip(sequence.get(&b.get_id())).is_some_and(|(i, j)| i < j)
        });
        let vals: Vec<_> = order.iter().map(|&j| placed[indices[j]].1.clone()).collect();
        for (&i, ev) in indices.iter().zip(vals) {
            placed[i].1 = ev;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwind_only_row_allows_the_normal_field_store_lookup() {
        let mut row = Row {
            facts: vec![Ev {
                location: Location::START,
                display_location: Location::START,
                caller_slot: None,
                transfer_paths: Vec::new(),
                end_places: Vec::new(),
                continuing_places: Vec::new(),
                fieldless_operand: None,
                drop_route: DropRoute::Unwind,
                place_local: None,
                occurrence: crate::facts::Occurrence::at(Location::START),
                col: 1,
                arm: None,
                fact: Fact::Drop {
                    path: "call_s".into(), how: "drops", at_reassign: false, early_exit: None, also_on_exits: Vec::new(),
                },
                order: (0, 0, 0),
            }],
            ..Row::default()
        };
        assert!(field_store_search_across_rows(&row, true));
        assert!(!field_store_search_across_rows(&row, false));
        row.facts[0].drop_route = DropRoute::Normal;
        assert!(!field_store_search_across_rows(&row, true));
    }
}
