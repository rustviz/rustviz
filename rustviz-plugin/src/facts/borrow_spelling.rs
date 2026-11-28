//! Source spelling of an already proven MIR borrow.

use rustc_hir::intravisit::{self, Visitor};
use rustc_hir::{ByRef, Expr, ExprKind, Mutability, Pat, PatKind};
use rustc_middle::mir::{Body, BorrowKind, Location, Place, ProjectionElem, Rvalue, StatementKind};
use rustc_middle::ty::{Ty, TyCtxt};
use rustc_span::Span;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct BorrowSpelling<'tcx> {
    expressions: BTreeMap<Span, (Mutability, Ty<'tcx>)>,
    patterns: BTreeMap<Span, Mutability>,
}

struct SourceBorrows<'tcx> {
    typeck: &'tcx rustc_middle::ty::TypeckResults<'tcx>,
    spelling: BorrowSpelling<'tcx>,
}

impl<'tcx> Visitor<'tcx> for SourceBorrows<'tcx> {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        if let ExprKind::AddrOf(rustc_hir::BorrowKind::Ref, mutability, _) = expr.kind {
            if !expr.span.from_expansion() {
                if let Some(ty) = self.typeck.expr_ty_opt(expr) {
                    self.spelling
                        .expressions
                        .insert(expr.span.with_parent(None), (mutability, ty));
                }
            }
        }
        intravisit::walk_expr(self, expr);
    }

    fn visit_pat(&mut self, pat: &'tcx Pat<'tcx>) {
        if let PatKind::Binding(mode, _, ident, _) = pat.kind {
            if let ByRef::Yes(_, mutability) = mode.0 {
                if !pat.span.from_expansion() {
                    self.spelling
                        .patterns
                        .insert(pat.span.with_parent(None), mutability);
                    self.spelling
                        .patterns
                        .insert(ident.span.with_parent(None), mutability);
                }
            }
        }
        intravisit::walk_pat(self, pat);
    }
}

impl<'tcx> BorrowSpelling<'tcx> {
    pub(super) fn collect(tcx: TyCtxt<'tcx>, owner: rustc_hir::def_id::LocalDefId) -> Self {
        let mut source = SourceBorrows {
            typeck: tcx.typeck(owner),
            spelling: Self::default(),
        };
        source.visit_body(tcx.hir_body_owned_by(owner));
        source.spelling
    }

    pub(super) fn is_written(&self, tcx: TyCtxt<'tcx>, body: &Body<'tcx>, at: Location) -> bool {
        let Some(statement) = body[at.block].statements.get(at.statement_index) else {
            return false;
        };
        let StatementKind::Assign(assignment) = &statement.kind else {
            return false;
        };
        let Rvalue::Ref(_, kind, place) = assignment.1 else {
            return false;
        };
        let span = statement.source_info.span.with_parent(None);
        let written = match kind {
            BorrowKind::Shared => Mutability::Not,
            BorrowKind::Mut {
                kind: rustc_middle::mir::MutBorrowKind::Default,
            } => Mutability::Mut,
            _ => return false,
        };
        if self.patterns.get(&span) == Some(&written) {
            // A match guard borrows the binding itself, at the same span
            // as the source borrow of the payload.
            return assignment
                .0
                .as_local()
                .is_some_and(|local| !body.local_decls[local].is_ref_for_guard());
        }
        let Some(&(mutability, ty)) = self.expressions.get(&span) else {
            return false;
        };
        let destination_ty = assignment.0.ty(body, tcx).ty;
        // A Deref adjustment can return a reference to a different type
        // and rustc can reborrow it with the original expression's span.
        if mutability != written
            || tcx.erase_and_anonymize_regions(ty)
                != tcx.erase_and_anonymize_regions(destination_ty)
        {
            return false;
        }
        // Coercion can use the written expression's span again when
        // reborrowing the reference just made by that expression.
        let through_written_borrow = place.projection.first() == Some(&ProjectionElem::Deref)
            && !body.local_decls[place.local].is_user_variable()
            && super::call_wording::operand_borrow(body, at, Place::from(place.local))
                .is_some_and(|(issue, _)| body.source_info(issue).span.with_parent(None) == span);
        !through_written_borrow
    }
}
