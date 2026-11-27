//! Connect the arms drawn for a HIR construct to the edges of its borrowck
//! MIR. Locations and successors are body-local. A match can make several
//! tests before committing to a body (notably a guard), so the mapping is
//! many-to-one and a test need not commit to any arm at all.

use super::rows::real_successor_blocks;
use super::schema::{SwitchEdge, SwitchExit};
use rustc_hir::{Arm, ExprKind, HirId, Node};
use rustc_middle::mir::{BasicBlock, Body, Location, Operand, StatementKind, TerminatorKind};
use rustc_middle::ty::TyCtxt;
use rustc_utils::BodyExt;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceArm {
    pub branch: HirId,
    pub index: usize,
}

/// The branch bodies enclosing a MIR instruction's compiler source scope.
/// A match guard is evaluated before entry into the arm body. A let-else
/// success can instead be witnessed by the assignment to its pattern.
/// For layout, declarations evaluated in a condition are drawn with
/// that condition's bindings. That association is never an entry edge.
pub(crate) fn enclosing_arms(tcx: TyCtxt<'_>, id: HirId, declaration_layout: bool) -> Vec<SourceArm> {
    let mut arms = Vec::new();
    let mut child = id;
    let mut body_arm = None;
    let declaration = matches!(tcx.hir_node(id), Node::Pat(_));
    for (parent, node) in tcx.hir_parent_iter(id) {
        let arm = match node {
            Node::Expr(e) => match e.kind {
                ExprKind::If(cond, then, _) if child == then.hir_id || declaration_layout && child == cond.hir_id => Some(0),
                ExprKind::If(_, _, Some(els)) if child == els.hir_id => Some(1),
                ExprKind::Match(_, arms, _) if body_arm == Some(child) => arms.iter().position(|a| a.hir_id == child),
                _ => None,
            },
            Node::Arm(a) => {
                body_arm = (child == a.body.hir_id
                    || declaration && child == a.pat.hir_id && (declaration_layout || a.guard.is_none())
                    || declaration_layout && a.guard.is_some_and(|guard| guard.hir_id == child))
                .then_some(parent);
                None
            }
            Node::LetStmt(stmt) if stmt.els.is_some() => {
                if stmt.els.is_some_and(|e| child == e.hir_id) {
                    Some(0)
                } else if child == stmt.pat.hir_id {
                    Some(1)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(index) = arm {
            arms.push(SourceArm { branch: parent, index });
        }
        child = parent;
    }
    arms
}

struct Decision {
    owner: HirId,
    /// A switch on the value of the complete boolean condition (before
    /// lowering a pattern or short-circuit expression into several tests).
    direct_bool: bool,
}

/// The construct whose condition this switch evaluates. The compiler sets
/// the lint root at each HIR-to-MIR lowering scope for the plugin.
fn decision(tcx: TyCtxt<'_>, body: &Body<'_>, at: Location) -> Option<Decision> {
    let id = body.location_to_hir_id(at);
    // then_else_break tests a Move of the temporary holding the whole
    // condition. A match used as that condition also has pattern switches
    // in the very same lint scope. Those test the scrutinee or a pattern
    // temporary, not the temporary for the match expression's result.
    let span = body.source_info(at).span;
    let tests_value = matches!(&body[at.block].terminator().kind,
        TerminatorKind::SwitchInt { discr: Operand::Move(p), .. }
        if p.as_local().is_some_and(|l| body.local_decls[l].source_info.span == span)
            && tcx.hir_span(id) == span);
    let mut child = id;
    let mut guard = false;
    for (owner, node) in std::iter::once((id, tcx.hir_node(id))).chain(tcx.hir_parent_iter(id)) {
        match node {
            Node::Expr(e) => match e.kind {
                ExprKind::If(cond, ..) if owner != id && cond.hir_id == child || owner == id && !tests_value => {
                    let direct_bool = tests_value && id == cond.hir_id
                        && !matches!(cond.kind, ExprKind::Let(_))
                        && body.source_info(at).span == cond.span;
                    return Some(Decision { owner, direct_bool });
                }
                ExprKind::Match(scrutinee, _, _) if owner == id && !tests_value || scrutinee.hir_id == child || guard => {
                    return Some(Decision { owner, direct_bool: false })
                }
                ExprKind::If(..) | ExprKind::Match(..) if owner != id => return None,
                ExprKind::Closure(..) => return None,
                _ => {}
            },
            Node::Arm(arm) => guard = arm.guard.is_some_and(|g| g.hir_id == child),
            Node::LetStmt(stmt)
                if stmt.els.is_some()
                    && (owner == id || child == stmt.pat.hir_id || stmt.init.is_some_and(|e| child == e.hir_id)) =>
            {
                return Some(Decision { owner, direct_bool: false })
            }
            _ => {}
        }
        child = owner;
    }
    None
}

/// rustc puts the candidate pattern's span on its pre-binding FalseEdge.
/// For an unguarded leaf the real edge commits to this arm, even when
/// the body is just a continue and has no instruction of its own.
fn candidate(tcx: TyCtxt<'_>, body: &Body<'_>, loc: Location) -> Option<SourceArm> {
    let term = body[loc.block].terminator();
    if loc != body.terminator_loc(loc.block) || !matches!(term.kind, TerminatorKind::FalseEdge { .. }) {
        return None;
    }
    let branch = body.location_to_hir_id(loc);
    if let Node::LetStmt(stmt) = tcx.hir_node(branch) {
        // A successful let-else without bindings can have no other MIR
        // instruction before the continuation following the statement.
        return (stmt.els.is_some() && stmt.pat.span == term.source_info.span).then_some(SourceArm { branch, index: 1 });
    }
    let Node::Expr(expr) = tcx.hir_node(branch) else { return None };
    let ExprKind::Match(_, arms, _) = expr.kind else { return None };
    let (index, arm) = unique_pattern(arms, |arm| arm.pat.span == term.source_info.span)?;
    arm.guard.is_none().then_some(SourceArm { branch, index })
}

fn unique_pattern<'hir>(arms: &'hir [Arm<'hir>], matches: impl Fn(&Arm<'hir>) -> bool) -> Option<(usize, &'hir Arm<'hir>)> {
    let mut matching = arms.iter().enumerate().filter(|(_, a)| matches(a));
    let first = matching.next()?;
    matching.next().is_none().then_some(first)
}

/// Source scopes can belong to the decision while MIR is still doing
/// bookkeeping in the selected successor. Consult the candidate boundary
/// and the switch's actual incoming edge for those instructions.
pub(crate) fn location_arms(tcx: TyCtxt<'_>, body: &Body<'_>, facts: &super::BodyFacts, at: Location) -> Vec<SourceArm> {
    let id = body.location_to_hir_id(at);
    let mut path = enclosing_arms(tcx, id, false);
    if let Some(a) = candidate(tcx, body, at) {
        path.push(a);
    }
    let incoming = super::rows::real_predecessors(body, at.block);
    let mut arms = incoming.iter().map(|&bb| {
        let pred = body.terminator_loc(bb);
        facts.entered(pred, Some(at.block.start_location())).or_else(|| candidate(tcx, body, pred))
    });
    if let Some(Some(a)) = arms.next() {
        if arms.all(|other| other == Some(a)) && (a.branch == id || tcx.hir_parent_id_iter(id).any(|parent| parent == a.branch)) {
            path.push(a);
        }
    }
    path
}

/// Follow a successor through bookkeeping and real gotos to the first
/// instruction in a source arm. Stop at another decision: passing the
/// first pattern test of a guarded arm does not establish guard success.
fn entered_arm(
    tcx: TyCtxt<'_>,
    body: &Body<'_>,
    owner: HirId,
    target: BasicBlock,
    bindings: &super::BodyBindings,
    scopes: &mut rustc_data_structures::fx::FxHashMap<HirId, Vec<SourceArm>>,
) -> Option<usize> {
    let mut block = target;
    let mut seen = BTreeSet::new();
    while seen.insert(block) {
        let data = &body.basic_blocks[block];
        for statement_index in 0..=data.statements.len() {
            let loc = Location { block, statement_index };
            if let Some(arm) = candidate(tcx, body, loc).filter(|a| a.branch == owner) {
                return Some(arm.index);
            }
            if let (Node::LetStmt(stmt), Some(rustc_middle::mir::StatementKind::Assign(assign))) =
                (tcx.hir_node(owner), data.statements.get(statement_index).map(|s| &s.kind))
            {
                let mut binds = false;
                stmt.pat.each_binding_or_first(&mut |_, id, _, _| {
                    binds |= bindings.declarations.get(&id) == Some(&assign.0.local);
                });
                if binds && assign.0.projection.is_empty() {
                    return Some(1);
                }
            }
            let id = body.location_to_hir_id(loc);
            // MIR lowers an absent else to an assignment of unit in the if
            // expression's own source scope (builder/expr/into.rs).
            if let Node::Expr(e) = tcx.hir_node(id) {
                if id == owner && matches!(e.kind, ExprKind::If(_, _, None))
                    && body.source_info(loc).span == e.span.shrink_to_hi()
                    && data.statements.get(statement_index).is_some_and(|s| matches!(&s.kind,
                        rustc_middle::mir::StatementKind::Assign(a) if matches!(&a.1,
                            rustc_middle::mir::Rvalue::Use(rustc_middle::mir::Operand::Constant(c)) if c.const_.ty().is_unit())))
                { return Some(1); }
            }
            let arms = scopes.entry(id).or_insert_with(|| enclosing_arms(tcx, id, false));
            if let Some(a) = arms.iter().find(|a| a.branch == owner) {
                return Some(a.index);
            }
        }
        // Only follow a real unconditional continuation. A backedge to
        // the test cannot commit to an arm on this iteration.
        let next = real_successor_blocks(body, block);
        let [target] = next.as_slice() else { break };
        if body.basic_blocks.dominators().dominates(*target, block) {
            break;
        }
        block = *target;
    }
    None
}

/// rustc lowers a string pattern to MatchCmp followed by a boolean test
/// of the call destination. Its source span identifies the pattern within
/// this match even when the true edge leads into a guard.
fn tested_pattern<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, switch: Location, discr: &Operand<'tcx>, owner: HirId) -> Option<usize> {
    let Node::Expr(e) = tcx.hir_node(owner) else { return None };
    let ExprKind::Match(_, arms, _) = e.kind else { return None };
    if switch.statement_index != 0 { return None; }
    let incoming = super::rows::real_predecessors(body, switch.block);
    if incoming.len() != 1 { return None; }
    let bb = *incoming.first()?;
    let term = body[bb].terminator();
    let TerminatorKind::Call { call_source: rustc_middle::mir::CallSource::MatchCmp, destination, target: Some(target), .. }
        = term.kind else { return None };
    if target != switch.block || discr.place() != Some(destination) || body.location_to_hir_id(body.terminator_loc(bb)) != owner {
        return None;
    }
    let (index, _) = unique_pattern(arms, |arm| {
        let mut found = false;
        arm.pat.walk_always(|p| found |= p.span == term.source_info.span);
        found
    })?;
    Some(index)
}

/// Read the Gotos emitted at arm completion by rustc's MIR builder.
/// The compiler puts the if's exact span on both of those Gotos. For
/// match it copies the arm's last SourceInfo (or uses the closing brace
/// for an empty final block, including the block after a return).
pub(super) fn conditional_joins<'hir>(
    tcx: TyCtxt<'_>, body: &Body<'_>, rows: &super::rows::RowModel,
    exprs: &[&'hir rustc_hir::Expr<'hir>], checked: &BTreeSet<BasicBlock>,
    bindings: &super::BodyBindings, switches: &[SwitchExit],
) -> Vec<(HirId, Location)> {
    let mut gotos = BTreeMap::<_, BTreeSet<_>>::new();
    for data in body.basic_blocks.iter() {
        let term = data.terminator();
        if let TerminatorKind::Goto { target } = term.kind {
            if !data.is_cleanup {
                gotos.entry(term.source_info.span).or_default().insert(target);
            }
        }
    }
    let mut spans = BTreeMap::<_, usize>::new();
    for e in exprs {
        *spans.entry(e.span).or_default() += 1;
    }
    let mut joins = Vec::new();
    for expr in exprs {
        let target = match expr.kind {
            ExprKind::If(..) | ExprKind::Block(_, Some(_)) if spans[&expr.span] == 1 => {
                let targets = gotos.get(&expr.span);
                targets.and_then(|targets| (targets.len() == 1).then(|| *targets.first().unwrap()))
            }
            ExprKind::Match(_, arms, rustc_hir::MatchSource::Normal) => {
                let brace = expr.span.with_lo(expr.span.hi() - rustc_span::BytePos(1));
                let bound_locals: Vec<BTreeSet<_>> = arms.iter().map(|arm| {
                    let mut locals = BTreeSet::new();
                    arm.pat.each_binding_or_first(&mut |_, id, _, _| {
                        locals.extend(bindings.declarations.get(&id).copied());
                    });
                    locals
                }).collect();
                let mut candidates = body.basic_blocks.indices().filter(|&target| {
                    let preds = &body.basic_blocks.predecessors()[target];
                    if preds.is_empty() || !checked.contains(&target) { return false; }
                    let destination = body.source_info(target.start_location()).span;
                    if arms.iter().any(|arm| arm.span.contains(destination.source_callsite())) { return false; }
                    let mut picked = BTreeSet::new();
                    let mut empty = 0;
                    let completes_match = preds.iter().all(|&p| {
                        let data = &body[p];
                        let term = data.terminator();
                        let at = body.terminator_loc(p);
                        let from_arm = |source: rustc_span::Span| {
                            arms.iter().position(|arm| arm.span.source_callsite().contains(source.source_callsite()))
                        };
                        // SimplifyCfg can fold the arm's empty Goto block
                        // into the successful edge of its final call.
                        if matches!(term.kind, TerminatorKind::Call { target: Some(to), .. } if to == target) {
                            return from_arm(term.source_info.span).is_some_and(|i| picked.insert(i));
                        }
                        if !matches!(term.kind, TerminatorKind::Goto { target: to } if to == target) {
                            return false;
                        }
                        if data.statements.is_empty() && term.source_info.span == brace {
                            empty += 1;
                            return true;
                        }
                        if rows.exit_only.contains(&at) || rows.early_exit.contains_key(&at) {
                            return false;
                        }
                        let Some(last) = data.statements.last() else { return false; };
                        if last.source_info.span != term.source_info.span
                            && !from_arm(term.source_info.span).is_some_and(|i| matches!(arms[i].body.kind, ExprKind::Loop(..)))
                        { return false; }
                        let local = match last.kind {
                            StatementKind::StorageDead(local) => Some(local),
                            _ => None,
                        };
                        let direct = from_arm(last.source_info.span).or_else(|| {
                            let local = local?;
                            bound_locals.iter().position(|locals| locals.contains(&local))
                                .or_else(|| from_arm(body.local_decls[local].source_info.span))
                        });
                        let traced = || {
                            // An arm can end with generated storage cleanup.
                            // Walk its MIR predecessors to a compiler arm
                            // scope or an edge committing to the same arm.
                            let mut work = vec![p];
                            let mut seen = BTreeSet::new();
                            let mut found = None;
                            while let Some(bb) = work.pop() {
                                if !seen.insert(bb) { continue; }
                                let loc = body.terminator_loc(bb);
                                let scope = body.location_to_hir_id(loc);
                                let arm = enclosing_arms(tcx, scope, false).into_iter()
                                    .find(|a| a.branch == expr.hir_id)
                                    .or_else(|| candidate(tcx, body, loc).filter(|a| a.branch == expr.hir_id));
                                if let Some(arm) = arm {
                                    if found.replace(arm.index).is_some_and(|i| i != arm.index) { return None; }
                                    continue;
                                }
                                let source = body.source_info(loc).span.source_callsite();
                                if !expr.span.contains(source) || bb == rustc_middle::mir::START_BLOCK { return None; }
                                for pred in super::rows::real_predecessors(body, bb) {
                                    let edge = SwitchEdge { switch: body.terminator_loc(pred), successor: bb };
                                    if let Some(arm) = entered(switches, edge).filter(|a| a.branch == expr.hir_id) {
                                        if found.replace(arm.index).is_some_and(|i| i != arm.index) { return None; }
                                    } else {
                                        work.push(pred);
                                    }
                                }
                            }
                            found
                        };
                        direct.or_else(traced).is_some_and(|i| picked.insert(i))
                    });
                    completes_match && (empty > 0 || picked.len() == arms.len() || destination == brace
                        || picked.len() > 1)
                });
                candidates.next().filter(|_| candidates.next().is_none())
            }
            _ => None,
        };
        // SimplifyCfg absorbs a join with one predecessor into that
        // predecessor. For a statement-valued conditional, MIR still
        // ends the storage of its result temporary at the closing brace.
        // The same temporary can also go dead on a returning edge.
        let boundary = target.map(BasicBlock::start_location).or_else(|| {
            let ExprKind::Match(scrutinee, _, rustc_hir::MatchSource::Normal) = expr.kind else { return None; };
            let brace = expr.span.with_lo(expr.span.hi() - rustc_span::BytePos(1));
            let mut drops = body.basic_blocks.iter_enumerated().filter_map(|(block, data)| {
                let term = data.terminator();
                let TerminatorKind::Drop { place, .. } = term.kind else { return None; };
                let at = body.terminator_loc(block);
                (checked.contains(&block) && term.source_info.span == brace
                    && !rows.exit_only.contains(&at) && !rows.early_exit.contains_key(&at)
                    && place.as_local().is_some_and(|l| body.local_decls[l].source_info.span == scrutinee.span))
                    .then_some(at)
            });
            drops.next().filter(|_| drops.next().is_none())
        }).or_else(|| {
            if !matches!(expr.kind, ExprKind::If(..) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) {
                return None;
            }
            let brace = expr.span.with_lo(expr.span.hi() - rustc_span::BytePos(1));
            let mut candidates = body.basic_blocks.iter_enumerated().filter(|(bb, _)| checked.contains(bb))
                .flat_map(|(block, data)| data.statements.iter().enumerate().filter_map(move |(statement_index, st)| {
                    let at = Location { block, statement_index };
                    if matches!(expr.kind, ExprKind::If(_, _, None)) {
                        return (st.source_info.span == expr.span.shrink_to_hi()
                            && matches!(&st.kind, StatementKind::Assign(a) if matches!(&a.1,
                                rustc_middle::mir::Rvalue::Use(Operand::Constant(c)) if c.const_.ty().is_unit())))
                            .then(|| at.successor_within_block());
                    }
                    let StatementKind::StorageDead(local) = st.kind else { return None; };
                    (statement_index > 0
                        && body.local_decls[local].source_info.span == expr.span
                        && st.source_info.span == brace
                        && !rows.exit_only.contains(&at)
                        && !rows.early_exit.contains_key(&at))
                        .then_some(at)
                }));
            candidates.next().filter(|_| candidates.next().is_none())
        }).or_else(|| {
            // A replacement may be the first effect after a folded RHS join.
            if !matches!(expr.kind, ExprKind::If(..) | ExprKind::Match(_, _, rustc_hir::MatchSource::Normal)) { return None; }
            let mut drops = body.basic_blocks.iter_enumerated().filter_map(|(block, data)| {
                let TerminatorKind::Drop { place, target, .. } = data.terminator().kind else { return None; };
                let StatementKind::Assign(a) = &body[target].statements.first()?.kind else { return None; };
                let rustc_middle::mir::Rvalue::Use(Operand::Move(result)) = a.1 else { return None; };
                (checked.contains(&block) && a.0 == place && result.as_local()
                    .is_some_and(|l| body.local_decls[l].source_info.span == expr.span))
                    .then(|| body.terminator_loc(block))
            });
            drops.next().filter(|_| drops.next().is_none())
        }).or_else(|| {
            // A conditional initializing a simple binding can have its
            // only completed arm merged into the caller's block. Rustc
            // still emits the initializer's FakeRead of that exact local.
            let Node::LetStmt(stmt) = tcx.hir_node(tcx.parent_hir_id(expr.hir_id)) else { return None; };
            if stmt.init.map(|e| e.hir_id) != Some(expr.hir_id) { return None; }
            let rustc_hir::PatKind::Binding(_, decl, _, None) = stmt.pat.kind else { return None; };
            let &local = bindings.declarations.get(&decl)?;
            let mut reads = body.basic_blocks.iter_enumerated().filter(|(bb, _)| checked.contains(bb))
                .flat_map(|(block, data)| data.statements.iter().enumerate().filter_map(move |(statement_index, st)| {
                    let StatementKind::FakeRead(read) = &st.kind else { return None; };
                    (matches!(read.0, rustc_middle::mir::FakeReadCause::ForLet(_))
                        && read.1.as_local() == Some(local)).then_some(Location { block, statement_index })
                }));
            reads.next().filter(|_| reads.next().is_none())
        });
        if let Some(at) = boundary { joins.push((expr.hir_id, at)); }
    }
    // The successful path of a let-else reaches the pattern stores.
    // When all of its bindings are made in one MIR block, read the
    // boundary immediately after its last such store. The builder uses
    // the binding's pattern span as the SourceInfo of its initial store.
    let by_let = switches.iter().filter_map(|exit| {
        let branch = exit.branch?;
        matches!(tcx.hir_node(branch), Node::LetStmt(stmt) if stmt.els.is_some()).then_some(branch)
    }).collect::<rustc_data_structures::fx::FxIndexSet<_>>();
    for id in by_let {
        let Node::LetStmt(stmt) = tcx.hir_node(id) else { continue; };
        let mut locals = BTreeSet::new();
        stmt.pat.each_binding_or_first(&mut |_, decl, _, _| {
            locals.extend(bindings.declarations.get(&decl).copied());
        });
        if locals.is_empty() { continue; }
        let mut writes = BTreeMap::new();
        for (block, data) in body.basic_blocks.iter_enumerated().filter(|(bb, _)| checked.contains(bb)) {
            for (statement_index, st) in data.statements.iter().enumerate() {
                if let StatementKind::Assign(a) = &st.kind {
                    if let Some(local) = a.0.as_local().filter(|l| locals.contains(l)
                        && stmt.pat.span.contains(st.source_info.span))
                    {
                        writes.entry(local).or_insert_with(Vec::new).push(Location { block, statement_index });
                    }
                }
            }
        }
        let Some(stores) = locals.iter().map(|local| match writes.get(local)?.as_slice() {
            [at] => Some(*at),
            _ => None,
        }).collect::<Option<Vec<_>>>() else { continue; };
        let Some(&last) = stores.iter().max() else { continue; };
        if stores.iter().all(|at| at.block == last.block) {
            joins.push((id, last.successor_within_block()));
        }
    }
    joins
}

pub(super) fn switch_exits<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, bindings: &super::BodyBindings) -> Vec<SwitchExit> {
    let mut exits = Vec::new();
    let mut scopes = Default::default();
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        if data.is_cleanup {
            continue;
        }
        let TerminatorKind::SwitchInt { discr, targets } = &data.terminator().kind else { continue };
        let switch = body.terminator_loc(bb);
        let test = decision(tcx, body, switch);
        let tested_pattern = test.as_ref().and_then(|d| tested_pattern(tcx, body, switch, discr, d.owner));
        let real: BTreeSet<_> = real_successor_blocks(body, bb).into_iter().collect();
        for successor in real {
            let arm = test.as_ref().and_then(|d| {
                if d.direct_bool && discr.ty(&body.local_decls, tcx).is_bool() {
                    let t = targets.target_for_value(1);
                    let f = targets.target_for_value(0);
                    if t != f {
                        return Some(usize::from(successor == f));
                    }
                }
                entered_arm(tcx, body, d.owner, successor, bindings, &mut scopes)
            });
            exits.push(SwitchExit { edge: SwitchEdge { switch, successor }, branch: test.as_ref().map(|d| d.owner), tested_pattern, arm });
        }
    }
    exits
}

/// The target's instructions belong to an arm if every real incoming
/// edge commits to that same arm (including a shared short-circuit exit).
pub(super) fn common_entry(
    exits: &[SwitchExit], body: &Body<'_>, target: BasicBlock, incoming: &BTreeSet<BasicBlock>,
) -> Option<SourceArm> {
    let mut work: Vec<_> = incoming.iter().map(|&bb| (bb, target)).collect();
    let mut seen = BTreeSet::new();
    let mut committed = None;
    while let Some((bb, to)) = work.pop() {
        if !seen.insert((bb, to)) { continue; }
        if matches!(body[bb].terminator().kind, TerminatorKind::SwitchInt { .. }) {
            let arm = entered(exits, SwitchEdge { switch: body.terminator_loc(bb), successor: to })?;
            if committed.replace(arm).is_some_and(|a| a != arm) { return None; }
        } else {
            // A failed guard can pass through StorageDead and a FalseEdge
            // before joining the pattern failure at the wildcard.
            if !matches!(body[bb].terminator().kind, TerminatorKind::Goto { .. } | TerminatorKind::FalseEdge { .. })
                || body.basic_blocks.dominators().dominates(to, bb) { return None; }
            let preds = super::rows::real_predecessors(body, bb);
            if preds.is_empty() { return None; }
            work.extend(preds.into_iter().map(|p| (p, bb)));
        }
    }
    committed
}

pub(super) fn entered(exits: &[SwitchExit], key: SwitchEdge) -> Option<SourceArm> {
    let e = &exits[exits.binary_search_by_key(&key, |e| e.edge).ok()?];
    Some(SourceArm { branch: e.branch?, index: e.arm? })
}

impl super::BodyFacts {
    /// The branch that this particular successor commits to. No arm is
    /// selected when the successor must still evaluate a guard.
    pub(crate) fn entered(&self, at: Location, to: Option<Location>) -> Option<SourceArm> {
        let to = to?;
        if to.statement_index != 0 {
            return None;
        }
        let key = SwitchEdge { switch: at, successor: to.block };
        entered(&self.switches, key)
    }

    /// A loan used by a pattern comparison can die on return from the
    /// call, before the guard or either arm body has executed.
    pub(crate) fn compared_pattern(&self, body: &Body<'_>, at: Location, to: Option<Location>) -> Option<SourceArm> {
        if at != body.terminator_loc(at.block) { return None; }
        let TerminatorKind::Call { call_source: rustc_middle::mir::CallSource::MatchCmp, target: Some(target), .. }
            = body[at.block].terminator().kind else { return None };
        let switch = body.terminator_loc(target);
        if to != Some(switch) { return None; }
        let i = self.switches.partition_point(|e| e.edge.switch < switch);
        let exit = self.switches.get(i).filter(|e| e.edge.switch == switch)?;
        Some(SourceArm { branch: exit.branch?, index: exit.tested_pattern? })
    }

    /// An outer reference's boundary on entry to this drawn arm. Its
    /// own copy can die while a different named slot carries the loan.
    /// An overwritten reference can instead release alternative loans
    /// together. Require agreement across every incoming edge and ID.
    pub(crate) fn reference_end_entering(
        &self, arm: SourceArm, local: rustc_middle::mir::Local, ids: &[usize],
    ) -> Option<(usize, &super::Ev)> {
        use super::Fact;
        if ids.is_empty() { return None; }
        let alternatives = ids.len() > 1 && ids.iter().all(|id| self.loans.iter().find(|l| l.loan == *id)
            .is_some_and(|loan| self.loans.iter().any(|other| ids.contains(&other.loan) && loan.alternative_to(other))));
        let mut entries = self.switches.iter().filter(|s| s.branch == Some(arm.branch) && s.arm == Some(arm.index));
        let entry = entries.next()?.edge;
        // Several tests (for example a let-chain) can enter a shared
        // arm. There is then no single switch occurrence to draw here.
        if entries.next().is_some() { return None; }
        let ends = self.rows.iter().flat_map(|r| r.facts.iter().map(move |e| (r.line, e)))
            .chain(self.inline_holder_ends.iter().map(|(row, e)| (*row, e)))
            .filter(|(_, e)| e.occurrence.at == entry.switch && e.occurrence.successor == Some(entry.successor.start_location())
                && e.end_places.iter().any(|p| p.local == local.index() && p.projection.is_empty()));
        let find = |id| ends.clone().find(|(_, e)| match &e.fact {
            Fact::HoldEnd { loan, .. } => loan == id && !e.continuing_places.is_empty(),
            Fact::BorrowEnd { loan, .. } => loan == id && alternatives,
            _ => false,
        });
        let (row, first) = find(&ids[0])?;
        ids[1..].iter().all(|id| find(id).is_some_and(|(_, e)| std::mem::discriminant(&e.fact) == std::mem::discriminant(&first.fact)))
            .then_some((row, first))
    }

    pub(crate) fn tests_of(&self, owner: HirId) -> BTreeSet<Location> {
        self.switches.iter().filter(|e| e.branch == Some(owner)).map(|e| e.edge.switch).collect()
    }
}
