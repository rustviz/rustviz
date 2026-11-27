//! Naming MIR places the way the source writes them, and deciding which
//! columns a loan restricts.
//!
//! Names: `s`, `p.a`, `t.0`, `b.a` (a field of a Box payload: the source
//! auto-derefs), `*b`, `v[_]` (any index), `(o as Some).0` (an enum
//! variant's field), `r.f` (a field behind a reference). Variables keep
//! the 2.x column names (`s`, `s₂`) from `binding_names`.

use crate::expr_visitor_utils::{body_binding_names, binding_name};
use super::schema::BodyBindings;
use rustc_middle::mir::{Body, Place, PlaceElem, ProjectionElem, VarDebugInfoContents};
use rustc_middle::ty::{Ty, TyCtxt, TyKind};

/// How a closure body sees a captured variable.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    ByValue,
    ByRef,
    ByMutRef,
    /// A unique immutable borrow (`*p = 1` in a closure, `p: &mut i32`
    /// not `mut`): the closure may not mutate `p`, but nobody else may
    /// use it meanwhile.
    ByUniqueRef,
}

pub struct Names<'tcx> {
    /// User variables that are whole locals.
    pub bindings: BodyBindings,
    /// Variables that live at a projection of a local: a closure's
    /// captures (`(*_1).0`, or `*((*_1).0)` for a capture by reference).
    pub aliases: Vec<(Place<'tcx>, String, Capture)>,
}

impl std::ops::Deref for Names<'_> {
    type Target = BodyBindings;
    fn deref(&self) -> &Self::Target { &self.bindings }
}

impl<'tcx> Names<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> Self {
        let owner = body.source.def_id().expect_local();
        let names = body_binding_names(&tcx, owner);
        let mut bindings = BodyBindings::default();
        let mut aliases = Vec::new();
        for vdi in body.var_debug_info.iter() {
            let VarDebugInfoContents::Place(place) = vdi.value else { continue };
            let span = vdi.source_info.span;
            if !place.projection.is_empty() {
                // A closure's captured variable: its debug info names the
                // place in the closure's environment.
                if place.local.as_usize() == 1 && is_closure_env(tcx, body) {
                    let field = place.projection.iter().find_map(|p| match p {
                        ProjectionElem::Field(f, _) => Some(f.index()), _ => None,
                    });
                    let Some(cap) = field.and_then(|f| tcx.closure_captures(owner).get(f)) else { continue };
                    // Projected captures have their own debug label. For a whole
                    // variable keep the outer binding's shadow suffix.
                    let label = if cap.place.projections.is_empty() {
                        binding_name(&tcx, cap.get_root_variable())
                    } else { vdi.name.to_string() };
                    aliases.push((place, label, capture_kind(cap.info.capture_kind)));
                }
                continue;
            }
            // A match guard sees its bindings through a reference of its
            // own (`RefForGuard`), named like the binding: not a column.
            if !body.local_decls[place.local].is_user_variable() || body.local_decls[place.local].is_ref_for_guard() {
                continue;
            }
            // Only bindings the user wrote: not `val` of a `?`, `iter` of
            // a `for`, `args` of `format_args!` (MIR's own).
            if span.from_expansion() && span.desugaring_kind().is_none()
                || span.desugaring_kind().is_some_and(|k| !matches!(k, rustc_span::DesugaringKind::ForLoop))
            {
                continue;
            }
            // A `for` loop's hidden iterator (`let mut iter = into_iter(..)`
            // of the desugaring, declared at the loop's head expression): not
            // the user's, even when the head is a variable named `iter`.
            let decl_span = body.local_decls[place.local].source_info.span;
            if span.desugaring_kind() == Some(rustc_span::DesugaringKind::ForLoop)
                && decl_span.desugaring_kind() == Some(rustc_span::DesugaringKind::ForLoop)
            {
                continue;
            }
            let Some(root) = body.local_decls[place.local].source_info.scope.lint_root(&body.source_scopes) else { continue };
            // For a simple formal argument MIR records the parameter's node.
            let pat = match tcx.hir_node(root) {
                rustc_hir::Node::Pat(p) => p,
                rustc_hir::Node::Param(p) => p.pat,
                _ => continue,
            };
            let rustc_hir::PatKind::Binding(_, id, ..) = pat.kind else { continue };
            let Some(label) = names.by_hir.get(&id) else { continue };
            bindings.declarations.insert(id, place.local);
            bindings.locals.insert(place.local, label.clone());
        }
        // Longest alias first, so the most specific one names a place.
        aliases.sort_by_key(|(p, _, _)| std::cmp::Reverse(p.projection.len()));
        for (local, name) in bindings.locals.iter() {
            bindings.columns.entry(name.clone()).or_default().push(*local);
        }
        Names { bindings, aliases }
    }

    /// The variable a place starts at, and how many projections that
    /// variable's own place takes (0 for a local, more for a capture).
    pub fn root(&self, place: Place<'tcx>) -> Option<(String, usize, Option<Capture>)> {
        if let Some(n) = self.locals.get(&place.local) {
            return Some((n.clone(), 0, None));
        }
        for (p, n, cap) in &self.aliases {
            if p.local == place.local
                && p.projection.len() <= place.projection.len()
                && place.projection.iter().zip(p.projection.iter()).all(|(a, b)| same_elem(a, b))
            {
                return Some((n.clone(), p.projection.len(), Some(*cap)));
            }
        }
        None
    }

    /// The place as the source would write it, if it starts at a user
    /// variable.
    pub fn name(&self, tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>) -> Option<String> {
        let (root, skip, _) = self.root(place)?;
        let mut ty = place_ty_prefix(tcx, body, place, skip);
        Some(name_projections(root, &mut ty, &place.projection[skip..]))
    }

    /// Describe projections that cannot be written as a named source place.
    pub(super) fn projected(&self, tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>) -> Option<super::ProjectedPlace> {
        use super::PlaceStep;
        let (_, skip, _) = self.root(place)?;
        if !place.projection[skip..].iter().any(|p| matches!(p, ProjectionElem::Index(_) |
            ProjectionElem::ConstantIndex { .. } | ProjectionElem::Subslice { .. })) {
            return None;
        }
        let first = (skip..place.projection.len()).find(|&i| matches!(place.projection[i],
            ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. } |
            ProjectionElem::Subslice { .. } | ProjectionElem::Downcast(..)))?;
        let prefix = Place { local: place.local, projection: tcx.mk_place_elems(&place.projection[..first]) };
        let base = self.name(tcx, body, prefix)?;
        let mut ty = place_ty_prefix(tcx, body, place, first);
        let mut variant = None;
        let mut steps = Vec::new();
        for elem in &place.projection[first..] {
            let step = match *elem {
                ProjectionElem::Deref => PlaceStep::Deref,
                ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. } => PlaceStep::Element,
                ProjectionElem::Subslice { .. } => PlaceStep::Subslice,
                ProjectionElem::Downcast(_, v) => {
                    variant = Some(v);
                    continue;
                }
                ProjectionElem::Field(field, _) => {
                    let f = match (ty.kind(), variant.take()) {
                        (TyKind::Adt(def, _), Some(v)) => {
                            let fields = &def.variant(v).fields;
                            if fields.len() == 1 && fields[field].name.as_str() == "0" {
                                steps.push(PlaceStep::Payload);
                                ty = project_ty(ty, elem);
                                continue;
                            }
                            fields[field].name.to_string()
                        }
                        (TyKind::Adt(def, _), None) if !def.is_enum() => def.non_enum_variant().fields[field].name.to_string(),
                        (TyKind::Tuple(_), _) => field.index().to_string(),
                        _ => return None,
                    };
                    PlaceStep::Field(f)
                }
                _ => return None,
            };
            steps.push(step);
            ty = project_ty(ty, elem);
        }
        (!steps.is_empty()).then_some(super::ProjectedPlace { base, steps })
    }

    /// A place in a MIR temporary, written from the temporary's source
    /// text: `(make_o().opt as Some).0`.
    pub fn name_in_temp(&self, body: &Body<'tcx>, place: Place<'tcx>, root: String) -> String {
        let mut ty = body.local_decls[place.local].ty;
        name_projections(root, &mut ty, &place.projection[..])
    }

    /// The reference a place is reached through, if it is behind one: the
    /// prefix before the place's last dereference of a reference (`r` for
    /// `*r`, `h.r` for `*h.r`, `r` for `r.f` with `r: &mut S`). A borrow
    /// of such a place is a reborrow "through" it.
    pub fn through(&self, tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>) -> Option<String> {
        let (_, skip, _) = self.root(place)?;
        let mut ty = place_ty_prefix(tcx, body, place, skip);
        let mut last = None;
        for (i, e) in place.projection.iter().enumerate().skip(skip) {
            if e == ProjectionElem::Deref && ty.is_ref() {
                last = Some(i);
            }
            ty = project_ty(ty, &e);
        }
        let i = last?;
        let prefix = Place { local: place.local, projection: tcx.mk_place_elems(&place.projection[..i]) };
        self.name(tcx, body, prefix)
    }
}

/// Two projection elements are the same step (a field's type differs
/// between debug info and the body by its region variables).
fn same_elem<'tcx>(a: PlaceElem<'tcx>, b: PlaceElem<'tcx>) -> bool {
    match (a, b) {
        (ProjectionElem::Field(f, _), ProjectionElem::Field(g, _)) => f == g,
        (ProjectionElem::Downcast(_, v), ProjectionElem::Downcast(_, w)) => v == w,
        _ => a == b,
    }
}

/// Does this loan select a field or element strictly inside the column?
/// Overlap alone does not imply containment (for example, union fields).
pub(super) fn borrows_part_of<'tcx>(borrowed: Place<'tcx>, column: Place<'tcx>) -> bool {
    let prefix = column.projection;
    borrowed.local == column.local
        && borrowed.projection.iter().zip(prefix.iter()).all(|(a, b)| same_elem(a, b))
        && borrowed.projection.iter().skip(prefix.len()).any(|p| matches!(p,
            ProjectionElem::Field(..) | ProjectionElem::Index(..) |
            ProjectionElem::ConstantIndex { .. } | ProjectionElem::Subslice { .. }
        ))
}

fn is_closure_env<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> bool {
    tcx.is_closure_like(body.source.def_id())
}

/// How the closure captured the variable at `place` (an environment
/// field, perhaps dereferenced): rustc's own capture analysis (the field's
/// type can't tell a unique immutable borrow from a mutable one).
fn capture_kind(kind: rustc_middle::ty::UpvarCapture) -> Capture {
    use rustc_middle::ty::{BorrowKind, UpvarCapture};
    match kind {
        UpvarCapture::ByRef(BorrowKind::Immutable) => Capture::ByRef,
        UpvarCapture::ByRef(BorrowKind::Mutable) => Capture::ByMutRef,
        UpvarCapture::ByRef(BorrowKind::UniqueImmutable) => Capture::ByUniqueRef,
        UpvarCapture::ByValue | UpvarCapture::ByUse => Capture::ByValue,
    }
}

/// The type after the first `n` projections of `place`.
fn place_ty_prefix<'tcx>(_tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: Place<'tcx>, n: usize) -> Ty<'tcx> {
    let mut ty = body.local_decls[place.local].ty;
    for e in place.projection.iter().take(n) {
        ty = project_ty(ty, &e);
    }
    ty
}

fn project_ty<'tcx>(ty: Ty<'tcx>, e: &PlaceElem<'tcx>) -> Ty<'tcx> {
    match e {
        ProjectionElem::Field(_, fty) => *fty,
        ProjectionElem::Deref => ty.builtin_deref(true).unwrap_or(ty),
        ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. } => match ty.kind() {
            TyKind::Array(t, _) | TyKind::Slice(t) => *t,
            _ => ty,
        },
        _ => ty,
    }
}

/// `root` followed by `proj`, written the way the source would.
fn name_projections<'tcx>(root: String, ty: &mut Ty<'tcx>, proj: &[PlaceElem<'tcx>]) -> String {
    let mut name = root;
    // A pending deref: written `*x` unless a field follows (`x.f`).
    let mut deref = 0usize;
    let mut downcast: Option<String> = None;
    for e in proj {
        match e {
            ProjectionElem::Deref => {
                deref += 1;
            }
            ProjectionElem::Field(f, _) => {
                let fname = match ty.kind() {
                    TyKind::Adt(def, _) if def.is_struct() || def.is_union() => {
                        def.non_enum_variant().fields[*f].name.to_string()
                    }
                    TyKind::Adt(def, _) if def.is_enum() => match &downcast {
                        Some(v) => {
                            let var = def.variants().iter().find(|vd| vd.name.as_str() == v);
                            var.map_or_else(|| f.index().to_string(), |vd| vd.fields[*f].name.to_string())
                        }
                        None => f.index().to_string(),
                    },
                    TyKind::Closure(..) => format!("<cap{}>", f.index()),
                    _ => f.index().to_string(),
                };
                // A deref followed by a field is the source's auto-deref.
                deref = 0;
                if let Some(v) = downcast.take() {
                    name = format!("({} as {}).{}", name, v, fname);
                } else {
                    name = format!("{}.{}", name, fname);
                }
            }
            ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. } | ProjectionElem::Subslice { .. } => {
                // `v[_]` indexes through `Vec`'s / a reference's deref.
                deref = 0;
                if let Some(v) = downcast.take() {
                    name = format!("({} as {})", name, v);
                }
                name = format!("{}[_]", name);
            }
            ProjectionElem::Downcast(Some(sym), _) => {
                if deref > 0 {
                    name = format!("{}{}", "*".repeat(deref), name);
                    deref = 0;
                }
                downcast = Some(sym.to_string());
            }
            _ => {}
        }
        *ty = project_ty(*ty, e);
    }
    if let Some(v) = downcast {
        name = format!("({} as {})", name, v);
    }
    if deref > 0 {
        name = format!("{}{}", "*".repeat(deref), name);
    }
    name
}

/// A projection the column model gives a column of its own: a struct or
/// tuple field, or a Box's payload.
pub fn column_projection<'tcx>(body: &Body<'tcx>, place: Place<'tcx>, skip: usize) -> bool {
    let mut ty = body.local_decls[place.local].ty;
    for (i, e) in place.projection.iter().enumerate() {
        if i >= skip {
            let ok = match e {
                ProjectionElem::Field(..) => true,
                ProjectionElem::Deref => ty.is_box(),
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        ty = project_ty(ty, &e);
    }
    true
}
