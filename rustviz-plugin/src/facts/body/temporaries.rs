//! MIR identities and effects for temporary columns, including places with no column.

use super::*;
use rustc_hir::{intravisit, Expr, ExprKind, HirId, Node, StmtKind};
use rustc_middle::middle::region::ScopeData;

#[derive(Default)]
struct Expressions<'tcx> {
    at: BTreeMap<Span, Vec<&'tcx Expr<'tcx>>>,
    iterator_scopes: BTreeMap<Span, Option<Span>>,
}

impl<'tcx> intravisit::Visitor<'tcx> for Expressions<'tcx> {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if let ExprKind::Match(_, [arm], rustc_hir::MatchSource::ForLoopDesugar) = expr.kind {
            if matches!(arm.pat.kind, rustc_hir::PatKind::Binding(..)) {
                self.iterator_scopes.entry(arm.pat.span.with_parent(None))
                    .and_modify(|scope| *scope = None).or_insert(Some(expr.span.source_callsite()));
            }
        }
        self.at.entry(expr.span.with_parent(None)).or_default().push(expr);
        intravisit::walk_expr(self, expr);
    }
}

fn extension(cx: &Cx<'_, '_>, scope: rustc_middle::middle::region::Scope) -> Option<TempExtension> {
    let tree = cx.tcx.region_scope_tree(cx.body.source.def_id().expect_local());
    let ScopeData::Remainder(first) = scope.data else { return None };
    let Node::Block(block) = cx.tcx.hir_node(scope.hir_id(tree)?) else { return None };
    let StmtKind::Let(binding) = block.stmts.get(first.index())?.kind else { return None };
    Some(TempExtension {
        initializer: binding.init?.hir_id,
        borrow: None,
        fields: String::new(),
        end: cx.r.line_hi(scope.span(cx.tcx, tree)),
    })
}

pub(super) fn collect<'tcx>(
    cx: &Cx<'_, 'tcx>, rt: &mut Flow<'_, '_, 'tcx>, runtime_sites: impl Fn(Location) -> Vec<Location>,
    promoted: &[Body<'tcx>], conditional_creation: impl Fn(HirId, &[Location]) -> bool,
    mut holders: BTreeMap<Local, Vec<String>>, for_iter: impl Fn(Local) -> bool,
) -> (BTreeMap<Local, TempInfo>, rustc_data_structures::fx::FxHashSet<HirId>) {
    let body = cx.body;
    let tcx = cx.tcx;
    let mut expressions = Expressions::default();
    if let Some(hir) = tcx.hir_maybe_body_owned_by(body.source.def_id().expect_local()) {
        intravisit::Visitor::visit_expr(&mut expressions, hir.value);
    }
    let env = rustc_middle::ty::TypingEnv::post_analysis(tcx, body.source.def_id());
    // More than one staging local can have precisely the same span and
    // type. Such locals still have effects, but neither identifies a column.
    let mut declarations = std::collections::HashMap::<_, Vec<Local>>::new();
    for (local, decl) in body.local_decls.iter_enumerated() {
        let span = decl.source_info.span;
        let vec_result = cx.vec_results.get(&span.ctxt().outer_expn()).is_some_and(|(_, place)| place.as_local() == Some(local));
        if local.index() > body.arg_count && !cx.is_user(Place::from(local))
            && (for_iter(local) || (!span.from_expansion() || vec_result) && !decl.is_user_variable())
            && !tcx.type_is_copy_modulo_regions(env, tcx.erase_and_anonymize_regions(decl.ty))
        {
            let ty = tcx.erase_and_anonymize_regions(decl.ty);
            declarations.entry((decl.source_info.span.with_parent(None), ty)).or_default().push(local);
        }
    }

    let mut temps = BTreeMap::new();
    for (local, decl) in body.local_decls.iter_enumerated() {
        let span = decl.source_info.span.with_parent(None);
        let ty = tcx.erase_and_anonymize_regions(decl.ty);
        let Some(locals) = declarations.get(&(span, ty)) else { continue };
        if !locals.contains(&local) { continue; }
        let expression = if locals.len() == 1 {
            let mut matching = expressions.at.get(&span).into_iter().flatten().filter(|e| {
                tcx.typeck(e.hir_id.owner).expr_ty_opt(e).is_some_and(|expr_ty|
                    tcx.erase_and_anonymize_regions(expr_ty) == ty)
            });
            matching.next().map(|e| e.hir_id).filter(|_| matching.next().is_none())
        } else { None };
        let tree = tcx.region_scope_tree(body.source.def_id().expect_local());
        let lifetime = expression.and_then(|id| tree.temporary_scope(id.local_id).temp_lifetime);
        let scope = lifetime.map(|s| s.span(tcx, tree))
            .or_else(|| expressions.iterator_scopes.get(&span).copied().flatten());
        let extension = lifetime.and_then(|s| extension(cx, s));
        let sm = tcx.sess.source_map();
        let source = if for_iter(local) { span } else { span.source_callsite() };
        let text = sm.span_to_snippet(source).unwrap_or_else(|_| "?".into());
        let what = if for_iter(local) { format!("into_iter({})", text.trim()) } else { text };
        let created_line = sm.lookup_char_pos(source.hi()).line;
        let mut held_by = holders.remove(&local).unwrap_or_default();
        held_by.sort();
        held_by.dedup();
        temps.insert(local, TempInfo {
            local, expression, scope, iterator_binding: for_iter(local).then_some(span), extended: extension.is_some(), extension, conditional: false,
            root_flags: BTreeMap::new(), has_root_drop: false,
            created_at: Vec::new(), moves: Vec::new(), drop_lines: Vec::new(),
            what: what.chars().take(40).collect(), ty: ty.to_string(), created_line, held_by,
        });
    }

    // MoveData also contains synthetic StorageDead moves. Only MIR operands transfer values.
    struct Moves<'tcx>(Vec<Place<'tcx>>);
    impl<'tcx> rustc_middle::mir::visit::Visitor<'tcx> for Moves<'tcx> {
        fn visit_operand(&mut self, operand: &Operand<'tcx>, _: Location) {
            if let Operand::Move(p) = operand { self.0.push(*p); }
        }
    }
    use rustc_middle::mir::visit::Visitor;
    let mut borrows = BTreeMap::<Local, Vec<(HirId, String)>>::new();
    for (bb, block) in body.basic_blocks.iter_enumerated() {
        if block.is_cleanup || runtime_sites(bb.start_location()).is_empty() { continue; }
        for i in 0..=block.statements.len() {
            let at = Location { block: bb, statement_index: i };
            let mut moves = Moves(Vec::new());
            let assignment = if let Some(stmt) = block.statements.get(i) {
                moves.visit_statement(stmt, at);
                let StatementKind::Assign(a) = &stmt.kind else { continue };
                if let Some(t) = a.0.as_local().and_then(|l| temps.get_mut(&l)) {
                    t.created_at.push(at);
                }
                if let Rvalue::Ref(_, kind, p) = a.1 {
                    if matches!(kind, BorrowKind::Fake(_)) || !temps.get(&p.local).is_some_and(|t| t.extended) { continue; }
                    // The source borrow anchors the drawing. The MIR operand identifies the lender.
                    for e in expressions.at.get(&stmt.source_info.span.with_parent(None)).into_iter().flatten() {
                        let anchor = if matches!(e.kind, ExprKind::AddrOf(rustc_hir::BorrowKind::Ref, ..)) {
                            Some(e.hir_id)
                        } else if temps[&p.local].expression == Some(e.hir_id) {
                            // The outer & pictures an overloaded index's receiver borrow.
                            let mut parents = tcx.hir_parent_iter(e.hir_id);
                            match (parents.next(), parents.next()) {
                                (Some((id, Node::Expr(index))), Some((_, Node::Expr(addr))))
                                    if matches!(index.kind, ExprKind::Index(base, ..) if base.hir_id == e.hir_id)
                                        && matches!(addr.kind, ExprKind::AddrOf(rustc_hir::BorrowKind::Ref, _, part) if part.hir_id == id) => Some(addr.hir_id),
                                _ => None,
                            }
                        } else { None };
                        if let Some(id) = anchor {
                            let fields = if p.projection.iter().all(|s| matches!(s, ProjectionElem::Field(..))) {
                                cx.names.name_in_temp(body, p, String::new())
                            } else { String::new() };
                            let anchor = (id, fields);
                            let candidates = borrows.entry(p.local).or_default();
                            if !candidates.contains(&anchor) { candidates.push(anchor); }
                        }
                    }
                }
                Some(&**a)
            } else {
                if let TerminatorKind::Call { destination, target: Some(_), .. } = block.terminator().kind {
                    if let Some(t) = destination.as_local().and_then(|l| temps.get_mut(&l)) {
                        t.created_at.push(body.terminator_loc(bb));
                    }
                }
                moves.visit_terminator(block.terminator(), at);
                None
            };
            for p in moves.0 {
                let Some(t) = temps.get_mut(&p.local) else { continue };
                let transfer = assignment.and_then(|(lhs, rv)| match rv {
                    Rvalue::Use(Operand::Move(_)) => Some(*lhs),
                    Rvalue::Aggregate(kind, ops) => {
                        let (field, _) = ops.iter_enumerated().find(|(_, op)| matches!(op, Operand::Move(src) if *src == p))?;
                        crate::facts::direct_holders::operand_place(tcx, body, *lhs, kind, field)
                    }
                    _ => None,
                });
                if tcx.type_is_copy_modulo_regions(env, tcx.erase_and_anonymize_regions(p.ty(body, tcx).ty)) { continue; }
                let LookupResult::Exact(root) = rt.md.rev_lookup.find(Place::from(p.local).as_ref()) else { continue };
                let sites = runtime_sites(at);
                let after = remainder(tcx, rt, &sites, true, root, Some(p)).value;
                let mut enclosing = Vec::new();
                for depth in 1..p.projection.len() {
                    let prefix = &p.projection[..depth];
                    if !matches!(prefix.last(), Some(ProjectionElem::Field(..) | ProjectionElem::ConstantIndex { .. })) { continue; }
                    let field = Place { local: p.local, projection: tcx.mk_place_elems(prefix) };
                    if let LookupResult::Exact(path) = rt.md.rev_lookup.find(field.as_ref()) {
                        enclosing.push((TransientPlace::new(tcx, field), remainder(tcx, rt, &sites, true, path, Some(p)).value));
                    }
                }
                t.moves.push(TempMove {
                    field: p.projection.iter().all(|step| matches!(step, ProjectionElem::Field(..)))
                        .then(|| cx.names.name_in_temp(body, p, String::new())),
                    at, from: TransientPlace::new(tcx, p), into: transfer.map(|p| TransientPlace::new(tcx, p)), after, enclosing,
                });
            }
        }
    }
    for (local, borrows) in borrows {
        let [(id, fields)] = borrows.as_slice() else { continue };
        if let Some(ext) = temps.get_mut(&local).and_then(|t| t.extension.as_mut()) {
            ext.borrow = Some(*id);
            ext.fields = fields.clone();
        }
    }
    for temp in temps.values_mut() {
        if let Some(ext) = &temp.extension {
            temp.conditional = conditional_creation(ext.initializer, &temp.created_at);
        }
    }
    temps.retain(|_, t| !t.created_at.is_empty() || !t.moves.is_empty());
    let mut promoted_operands = rustc_data_structures::fx::FxHashSet::default();
    for decl in promoted.iter().flat_map(|body| body.local_decls.iter().skip(1)) {
        let span = decl.source_info.span.with_parent(None);
        let ty = tcx.erase_and_anonymize_regions(decl.ty);
        for expr in expressions.at.get(&span).into_iter().flatten() {
            if tcx.typeck(expr.hir_id.owner).expr_ty_opt(expr)
                .is_some_and(|t| tcx.erase_and_anonymize_regions(t) == ty) {
                promoted_operands.insert(expr.hir_id);
            }
        }
    }
    (temps, promoted_operands)
}

/// A downcast move selects its active variant. Untracked siblings inherit the parent's init bit.
fn has_glue<'tcx>(
    tcx: TyCtxt<'tcx>, rt: &mut Flow<'_, '_, 'tcx>, sites: &[Location], after: bool,
    place: Place<'tcx>, moved: Option<Place<'tcx>>,
) -> bool {
    let body = rt.body;
    let ty = place.ty(body, tcx).ty;
    let env = rustc_middle::ty::TypingEnv::post_analysis(tcx, body.source.def_id());
    if !ty.needs_drop(tcx, env) { return false; }
    let LookupResult::Exact(path) = rt.md.rev_lookup.find(place.as_ref()) else { return true };
    let state = rt.state(sites, after, path);
    if !state.may_init { return false; }
    if state.sub_moved.is_empty() && state.sub_maybe.is_empty() { return true; }
    match *ty.kind() {
        TyKind::Tuple(fields) => fields.iter().enumerate().any(|(i, ty)|
            has_glue(tcx, rt, sites, after, tcx.mk_place_field(place, i.into(), ty), moved)),
        TyKind::Adt(def, args) if !def.has_dtor(tcx) && !def.is_box() && !def.is_union() => {
            let active = moved.and_then(|p| p.projection.strip_prefix(&place.projection[..]))
                .and_then(|tail| match tail.first() {
                    Some(ProjectionElem::Downcast(_, v)) => Some(*v),
                    _ => None,
                }).or_else(|| def.is_enum().then(|| def.variants().indices().find(|&v| {
                    let down = tcx.mk_place_downcast(place, def, v);
                    let LookupResult::Exact(path) = rt.md.rev_lookup.find(down.as_ref()) else { return false };
                    let st = rt.state(sites, after, path);
                    st.may_init && !st.may_uninit
                })).flatten());
            def.variants().iter_enumerated().filter(|(v, _)| active.is_none_or(|a| a == *v)).any(|(v, variant)| {
                let base = if def.is_enum() { tcx.mk_place_downcast(place, def, v) } else { place };
                variant.fields.iter_enumerated().any(|(i, f)|
                    has_glue(tcx, rt, sites, after, tcx.mk_place_field(base, i, f.ty(tcx, args)), moved))
            })
        }
        TyKind::Array(_, n) => {
            let mut parts = BTreeSet::new();
            let mut next = rt.md.move_paths[path].first_child;
            while let Some(i) = next {
                let child = &rt.md.move_paths[i];
                next = child.next_sibling;
                let Some(ProjectionElem::ConstantIndex { offset, from_end: false, .. }) = child.place.projection.last() else { return true };
                parts.insert(*offset);
                if has_glue(tcx, rt, sites, after, child.place, moved) { return true; }
            }
            n.try_to_target_usize(tcx) != Some(parts.len() as u64)
        }
        // A Box can still free its allocation.
        _ => true,
    }
}

pub(super) fn remainder<'tcx>(
    tcx: TyCtxt<'tcx>, rt: &mut Flow<'_, '_, 'tcx>, sites: &[Location], after: bool,
    path: MovePathIndex, moved: Option<Place<'tcx>>,
) -> TempRemainder {
    let state = rt.state(sites, after, path);
    let place = rt.md.move_paths[path].place;
    let value = if !state.may_init || !has_glue(tcx, rt, sites, after, place, moved) {
        TempValue::Empty
    } else if state.may_uninit {
        TempValue::Conditional
    } else if state.sub_moved.is_empty() && state.sub_maybe.is_empty() {
        TempValue::Whole
    } else {
        TempValue::Remaining
    };
    let places = |paths: Vec<MovePathIndex>| paths.into_iter()
        .map(|p| TransientPlace::new(tcx, rt.md.move_paths[p].place)).collect();
    TempRemainder { value, absent: places(state.sub_moved), maybe_absent: places(state.sub_maybe) }
}

/// Read a destructor on its particular exit, cleanup or normal CFG inputs.
pub(super) fn drop_event<'tcx>(
    cx: &Cx<'_, 'tcx>, rt: &mut Flow<'_, '_, 'tcx>, temp: &mut TempInfo,
    loc: Location, place: Place<'tcx>, sites: &[Location], after: bool,
    exit: Option<(Span, Location, Option<Location>)>, route: DropRoute, flag: bool,
    moved_entry: &BTreeMap<BasicBlock, BTreeSet<MovePathIndex>>,
) -> Option<(usize, Ev)> {
    let LookupResult::Exact(path) = rt.md.rev_lookup.find(place.as_ref()) else { return None };
    let mut remainder = remainder(cx.tcx, rt, sites, after, path, None);
    // Variant refinement marks inactive payloads uninitialized without
    // moving them. Keep only absences with a reaching move on this edge.
    if place.ty(cx.body, cx.tcx).ty.ty_adt_def().is_some_and(|d| d.is_enum())
        && (!remainder.absent.is_empty() || !remainder.maybe_absent.is_empty()) {
        let moved: BTreeSet<_> = sites.iter().flat_map(|&at|
            may_after(moved_entry, &|st, l| moved_transfer(rt.md, rt.body, st, l), at, after)
        ).map(|p| TransientPlace::new(cx.tcx, rt.md.move_paths[p].place)).collect();
        remainder.absent.retain(|p| moved.contains(p));
        remainder.maybe_absent.retain(|p| moved.contains(p));
        if remainder.value == TempValue::Remaining && remainder.absent.is_empty() && remainder.maybe_absent.is_empty() {
            remainder.value = TempValue::Whole;
        }
    }
    if remainder.value == TempValue::Empty { return None; }
    temp.has_root_drop |= place.projection.is_empty() && route != DropRoute::Unwind;
    let (mut row, mut ev) = cx.place_ev(exit.map_or(loc, |(_, at, _)| at), place.local, Fact::TempDrop {
        what: temp.what.clone(), created_line: temp.created_line, extended: temp.extended,
        early_exit: exit.filter(|_| route != DropRoute::Unwind).map(|(site, _, _)| cx.r.line_lo(site)),
        place: TransientPlace::new(cx.tcx, place), remainder, flag,
    });
    ev.occurrence = Occurrence { at: loc, exit: exit.map(|(_, at, _)| at), successor: exit.and_then(|e| e.2) };
    ev.order = cx.rows.order(loc);
    ev.drop_route = route;
    if place.projection.is_empty() {
        let site = exit.map(|e| e.0.with_parent(None));
        temp.root_flags.entry(site).and_modify(|guarded| *guarded &= flag).or_insert(flag);
    }
    if let Some((site, at, _)) = exit {
        row = cx.r.line_lo(site);
        ev.display_location = at;
        ev.col = cx.rows.col.get(&at).copied().unwrap_or(0);
        ev.arm = cx.rows.arm_of(at);
    }
    if route != DropRoute::Unwind { temp.drop_lines.push(row); }
    Some((row, ev))
}
