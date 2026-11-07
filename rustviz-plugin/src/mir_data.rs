//! Ideally we would like to do all of our analysis in the MIR since it is a lot simpler than the HIR
//! since events like drops, borrows and moves are stated explicitly. Additionally, the MIR is less complicated.
//! If you'd like to work on converting more and more of the RustViz logic from the HIR to the MIR I would recommend reading:
//! https://rustc-dev-guide.rust-lang.org/ especially the sections on the MIR, dataflow analysis, borrow checker, drop elaboration
//! Also read through this: https://github.com/rust-lang/polonius/blob/master/book/src/SUMMARY.md
//! Unfortunately though, I don't believe it would be possible to do everything in the MIR, for example: how would 
//! you figure out when a copy occurs? You can't use the assignment operator - since a single HIR instr can be compiled down into
//! multiple MIR instructions.

use crate::{expr_visitor::*, expr_visitor_utils::{binding_names, span_to_line, written_name}};
use log::info;
use rustc_middle::mir::{Location, Local, Operand, ProjectionElem, VarDebugInfoContents};
use rustc_middle::mir::visit::Visitor;
use rustc_middle::ty::Ty;
use std::collections::HashMap;
use rustc_utils::mir::{body::BodyExt, place::PlaceExt};
use rustc_utils::SpanExt;
use rustc_borrowck::consumers::{BodyWithBorrowckFacts, BorrowIndex, Borrows, RustcFacts};
use rustc_mir_dataflow::Analysis;
use rustc_span::Span;
use polonius_engine::{Algorithm, Output, FactTypes};
type Loan = <RustcFacts as FactTypes>::Loan;
type Point = <RustcFacts as FactTypes>::Point;

pub struct MIRBorrowData {
    pub borrow_idx: usize,
    pub region: (usize, usize),
    pub borrowed_place: Option<String>,
    pub assigned_place: Option<String>,
    /// Source span of the borrow expression (`&x`) this loan was issued
    /// at; see `RefData::borrow_span`.
    pub span: Span,
    // could add more useful members if necessary like kind of mutable borrow, regionvid, etc...

}


/// Finds a move of a field out of `*local` (`move ((*_1).0)`).
struct BoxFieldMoves { local: Local, found: bool }

impl<'tcx> Visitor<'tcx> for BoxFieldMoves {
    fn visit_operand(&mut self, op: &Operand<'tcx>, loc: Location) {
        if let Operand::Move(place) = op {
            if place.local == self.local
                && matches!(place.projection.first(), Some(ProjectionElem::Deref))
                && matches!(place.projection.get(1), Some(ProjectionElem::Field(..)))
            {
                self.found = true;
            }
        }
        self.super_operand(op, loc);
    }
}

/// Finds a move of the whole contents of `*local` (`move (*_1)`).
struct BoxContentsMoves { local: Local, found: bool }

impl<'tcx> Visitor<'tcx> for BoxContentsMoves {
    fn visit_operand(&mut self, op: &Operand<'tcx>, loc: Location) {
        if let Operand::Move(place) = op {
            if place.local == self.local && matches!(place.projection.as_slice(), [ProjectionElem::Deref]) {
                self.found = true;
            }
        }
        self.super_operand(op, loc);
    }
}

impl <'a, 'tcx> ExprVisitor<'a, 'tcx> {
    /// Is `name`, a `Box<T>`, emptied by a move of its whole contents
    /// somewhere in this body (`take(*b)`)? Its heap allocation is
    /// still freed at scope end (#609).
    pub fn box_contents_moved_out(&self, name: &str, ty: Ty<'tcx>) -> bool {
        if !ty.is_box() { return false; }
        let names = self.mir_local_names(self.bwf);
        #[expect(clippy::disallowed_methods, reason = "a name is bound to one local; min() makes order irrelevant")]
        let Some(local) = names.iter().filter(|(_, n)| n.as_str() == name).map(|(l, _)| *l).min() else { return false };
        let mut v = BoxContentsMoves { local, found: false };
        v.visit_body(&self.bwf.body);
        v.found
    }

    /// For a `Box<T>` variable `name` whose `T` has field columns and
    /// from which a field is moved out somewhere in this body
    /// (`take(b.name)`): `T`. Rust allows that move out of a `Box`
    /// only, not out of other pointers (#602).
    pub fn box_with_field_moved_out(&self, name: &str, ty: Ty<'tcx>) -> Option<Ty<'tcx>> {
        if !ty.is_box() { return None; }
        let inner = ty.boxed_ty()?;
        if !self.has_member_columns(inner) { return None; }
        let names = self.mir_local_names(self.bwf);
        #[expect(clippy::disallowed_methods, reason = "a name is bound to one local; min() makes order irrelevant")]
        let local = names.iter().filter(|(_, n)| n.as_str() == name).map(|(l, _)| *l).min()?;
        let mut v = BoxFieldMoves { local, found: false };
        v.visit_body(&self.bwf.body);
        v.found.then_some(inner)
    }

    // these are just helper functions copied from aquascope permissionctxt
    fn point_to_location(&self, p: Point, body_with_facts: &BodyWithBorrowckFacts) -> Location {
        body_with_facts.location_table.as_ref().unwrap().to_location(p)
    }

    fn location_to_span(&self, l: Location, body_with_facts: &BodyWithBorrowckFacts) -> Span {
        let span = body_with_facts.body.source_info(l).span;
        span.as_local(body_with_facts.body.span).unwrap_or(span)
    }

    fn location_to_line(&self, l: Location, body_with_facts: &BodyWithBorrowckFacts) -> usize {
        span_to_line(&self.location_to_span(l, body_with_facts), &self.tcx)
    }

    fn src_name_of_local(&self, l: Option<Local>, l_map: &HashMap<Local, String>) -> Option<String> {
        match l {
            Some(loc) => {
                match l_map.get(&loc) {
                    Some(name) => Some(name.clone()),
                    None => None
                }
            }
            None => None
        }
    }


    /// Name of each user variable's MIR local, as used for its RAP
    /// (`s₂` for a second `let s`, see `binding_name`). rustc_utils'
    /// `debug_info_name_map` is keyed by the source name, so it kept
    /// only one of two shadowing `s`es; here each local is matched to
    /// its binding through the binding's span.
    fn mir_local_names(&self, body_with_facts: &BodyWithBorrowckFacts) -> HashMap<Local, String> {
        let body = &body_with_facts.body;
        let owner = self.tcx.local_def_id_to_hir_id(body.source.def_id().expect_local()).owner;
        let names = binding_names(&self.tcx, owner);
        let mut res: HashMap<Local, String> = HashMap::new();
        for vdi in body.var_debug_info.iter() {
            let VarDebugInfoContents::Place(place) = vdi.value else { continue };
            let span = vdi.source_info.span;
            let ident = vdi.name.as_str();
            let name = names.by_span.iter()
                .find(|(sp, n)| span.contains(*sp)
                    && written_name(n) == ident)
                .map_or_else(|| ident.to_owned(), |(_, n)| n.clone());
            // If two names share a Local (closure captures all live in
            // `_1`), the same one wins on every run (#211): the greatest,
            // as before.
            match res.get(&place.local) {
                Some(prev) if *prev >= name => {}
                _ => { res.insert(place.local, name); }
            }
        }
        res
    }

    /// The last line at which each loan is in scope for rustc's NLL
    /// borrow checker (#644): the `Borrows` dataflow rustc's borrowck
    /// runs, whose state before a statement is the set of loans an
    /// access there conflicts with. Polonius' `loan_live_at` is more
    /// precise (location-sensitive subset relations) and can end a loan
    /// earlier than rustc does, which would draw a lender free where
    /// rustc still rejects using it.
    fn nll_loan_end_lines(&self, body_with_facts: &BodyWithBorrowckFacts<'tcx>) -> HashMap<BorrowIndex, usize> {
        let body = &body_with_facts.body;
        let borrows = Borrows::new(self.tcx, body, &body_with_facts.region_inference_context, &body_with_facts.borrow_set);
        let mut cursor = borrows.iterate_to_fixpoint(self.tcx, body, None).into_results_cursor(body);
        let mut ends: HashMap<BorrowIndex, usize> = HashMap::new();
        for (block, data) in body.basic_blocks.iter_enumerated() {
            for statement_index in 0..=data.statements.len() {
                let location = Location { block, statement_index };
                cursor.seek_before_primary_effect(location);
                if cursor.get().is_empty() { continue; }
                let line = self.location_to_line(location, body_with_facts);
                for b in cursor.get().iter() {
                    let e = ends.entry(b).or_insert(line);
                    if line > *e { *e = line; }
                }
            }
        }
        ends
    }

    // To get this to really work we would need to do a dataflow analysis to establish which temporaries 
    // refer to the same piece of data. Since the way the MIR is SSA.
    // For example:
    //  Let mut a = &b;
    //  a = &c;
    // compiles to: (where  _1: "b", _2: "c", "_3: a")
    // _3 = &'?2 _1; <-- let mut a = &b
    // _5 = &'?3 _2;     <- |
    // _4 = &'?4 (*_5);     |
    // _3 = move _4;     <- | All these instructions are used for a = &c 
    // notice how we generate new temporaries (_4 and _5)
    // Consequently, when we get the borrow data we see a borrow between (_5 and _2)
    // Which doesn't allow us to trace it back to the borrowed and assigned place in the source code (c, a)
    pub fn gather_borrow_data(&self, body_with_facts: &BodyWithBorrowckFacts<'tcx>) -> Vec<MIRBorrowData>{
        // compute output facts using polonius
        let p_output = Output::compute(&body_with_facts.input_facts.as_ref().unwrap(), Algorithm::Naive, true);

        // compute local to name map
        let loc_to_source_name = self.mir_local_names(body_with_facts);
        
        
        // compute the loan regions
        let mut loan_regions: HashMap<Loan, (usize, usize)> = HashMap::new();
        // `loan_live_at` is an FxHashMap (fixed but arbitrary order).
        #[expect(clippy::disallowed_methods, reason = "keeps a min/max line per loan; order doesn't matter")]
        p_output.loan_live_at.iter().for_each(|(point, loans)| {
          loans.iter().for_each(|loan| {
            loan_regions.entry(*loan).and_modify(|(l1, l2)|{
              let l = self.point_to_location(*point, &body_with_facts);
              let line = self.location_to_line(l, &body_with_facts);
              if line < *l1 { *l1 = line }
              else if line > *l2 { *l2 = line}
            })
            .or_insert_with(|| {
              let l = self.point_to_location(*point, &body_with_facts);
              let line = self.location_to_line(l, &body_with_facts);
              (line, line)
            });
          });
        });
        
        let nll_ends = self.nll_loan_end_lines(body_with_facts);
        let mut res: Vec<MIRBorrowData> = Vec::new();

        // refine loan regions (when testing they seem to be off sometimes) by looping through loan assignments
        for (_region, b_idx, location_idx) in body_with_facts.input_facts.as_ref().unwrap().loan_issued_at.iter() {
            let location = body_with_facts.location_table.as_ref().unwrap().to_location(*location_idx);
            let assignment_line = self.location_to_line(location, &body_with_facts);
            match body_with_facts.borrow_set.location_map().get(&location) {
              Some(b_data) => {
                // Where rustc's borrow checker ends the loan (#644);
                // Polonius' end only for a loan NLL never has in scope
                // after the statement that makes it.
                let end_line = nll_ends.get(b_idx).copied()
                  .unwrap_or_else(|| loan_regions.get(b_idx).map_or(assignment_line, |r| r.1));
                // println!("borrow_data for location {:#?} : {:#?}", location, b_data);
                // For a borrow of `r.a.b`, MIR's `borrowed_place`
                // is `r` with projections [Field(a), Field(b)].
                // `local_or_deref_local()` returns None for any
                // projection that isn't a single deref, which is why
                // ref-lifetime refinement used to skip every borrow
                // of a struct field. Use the base `.local` directly
                // so we always get the source name; `borrow_match`
                // matches against either the exact lender name or a
                // path prefix terminated by `.`.
                let b_place = Some(b_data.borrowed_place().local);
                let a_place = Some(b_data.assigned_place().local);
                let b_name = self.src_name_of_local(b_place, &loc_to_source_name);
                let a_name = self.src_name_of_local(a_place, &loc_to_source_name);

                // (A loan that is never live, like the `&s` MIR takes of a
                // `ref s` binding for a guard that doesn't use `s`, ends
                // where it's issued: `assignment_line` above.)
                res.push(MIRBorrowData {borrow_idx: b_idx.index(), 
                                        region: (assignment_line, end_line),
                                        borrowed_place: b_name, 
                                        assigned_place: a_name,
                                        span: self.location_to_span(location, &body_with_facts)});
              }
              None => { }
            }
          }

            // debugging info
            info!("locals map {:#?}", loc_to_source_name);
            info!("body string {}", self.bwf.body.to_string(self.tcx).unwrap());
            for (region, b_idx, location_idx) in body_with_facts.input_facts.as_ref().unwrap().loan_issued_at.iter() {
                info!("loan issued at {:#?}", (region, b_idx, location_idx));
                let location = body_with_facts.location_table.as_ref().unwrap().to_location(*location_idx);
                match body_with_facts.borrow_set.location_map().get(&location) {
                    Some(b_data) => {
                    info!("borrow_data for location {:#?} : {:#?}", location, b_data);
                    let b_place = b_data.borrowed_place().local_or_deref_local();
                    let a_place = b_data.assigned_place().local_or_deref_local();
                    // Log places with their Debug form, not rustc_utils'
                    // `PlaceExt::to_string`: that helper hits
                    // `unimplemented!("Union")` on a union-field projection
                    // (`&b.f` with `b: SomeUnion`), and because the logger
                    // runs at Info this debug line would panic the plugin.
                    info!("borrowed_place: {:?}, as local: {:#?}", b_data.borrowed_place(), b_place);
                    info!("is source visible? {}", b_data.borrowed_place().is_source_visible(self.tcx, &body_with_facts.body));
                    // RegionInferenceContext's per-region accessors are pub(crate) in 1.91;
                    // log just the region id here. If we need the NLL origin, we can lift
                    // this through rustc_utils' consumer wrappers later.
                    info!("region inference context: region {:?}", region);
                    if b_place.is_some() { 
                        let b_loc = body_with_facts.body.local_decls.get(b_place.unwrap()).unwrap();
                        info!("local decl {:#?}", b_loc); 
                        //println!("span {}", b_loc.source_info.span.to_string(self.tcx));
                    }
                    info!("assigned place {:?}, as local {:#?}", b_data.assigned_place(), a_place);
                    info!("is source visible? {}", b_data.assigned_place().is_source_visible(self.tcx, &body_with_facts.body));
                    if a_place.is_some() { 
                        let a_loc =  body_with_facts.body.local_decls.get(a_place.unwrap()).unwrap();
                        info!("local decl {:#?}", a_loc);
                        //println!("span {}", a_loc.source_info.span.to_string(self.tcx));
                    }

                    // origins live at
                    // println!("origins live at {:#?}", p_output.origins_live_at(*location_idx));
                    // println!("loans in scope at {:#?}", p_output.loans_in_scope_at(*location_idx));
                    // println!("origin contains loans at {:#?}", p_output.origin_contains_loan_at(*location_idx));
                    // let expr = self.tcx.hir_expect_expr(borrow_data.body.location_to_hir_id(location.clone()));
                    // println!("line {}\n\n", self.span_to_line(&expr.span));
                    }
                    None => { println!("no borrow data found for location"); }
                }
            }

          res
    }


    /// The last line in `lines` (first, last) on which the variable
    /// `name` (its RAP name, see `mir_local_names`) is live, from
    /// Polonius' liveness: its last use, or the end of a loop it's used
    /// in.
    pub fn last_live_line(&self, name: &str, lines: (usize, usize)) -> Option<usize> {
        let bwf = self.bwf;
        let p_output = Output::compute(bwf.input_facts.as_ref().unwrap(), Algorithm::Naive, true);
        let names = self.mir_local_names(bwf);
        #[expect(clippy::disallowed_methods, reason = "only collects the locals named `name`; order doesn't matter")]
        let locals: Vec<Local> = names.iter().filter(|(_, n)| n.as_str() == name).map(|(l, _)| *l).collect();
        #[expect(clippy::disallowed_methods, reason = "only takes the greatest line")]
        let last = p_output.var_live_on_entry.iter()
            .filter(|(_, vars)| vars.iter().any(|v| locals.contains(v)))
            .map(|(p, _)| self.location_to_line(self.point_to_location(*p, bwf), bwf))
            .filter(|l| lines.0 <= *l && *l <= lines.1)
            .max();
        last
    }

    // would be nice to use MIR to figure out where drops occur rather than our own internal state logic
    pub fn gather_drop_data(&self, body_with_facts: &BodyWithBorrowckFacts) -> HashMap<String, usize> {
        // compute local to name map
        let loc_to_source_name = self.mir_local_names(body_with_facts);

        let mut res = HashMap::new();
        for (local, loc_idx) in body_with_facts.input_facts.as_ref().unwrap().var_dropped_at.iter() {
            let line = self.location_to_line(self.point_to_location(*loc_idx, body_with_facts), body_with_facts);
            match loc_to_source_name.get(local) {
                Some(s) => {
                    res.insert(s.to_owned(), line);
                }
                None => {}
            }
        }
        res
    }


    /// The loan issued at exactly the `&x` expression `r` was assigned
    /// from (#233). Only the kill line is used, as in `borrow_match`.
    pub fn borrow_span_match(r: &RefData, b: &MIRBorrowData) -> bool {
        r.borrow_span.is_some_and(|sp| sp == b.span)
    }

    // Helper function to help refine loan regions that we compute in HIR
    pub fn borrow_match(r: &RefData, b: &MIRBorrowData) -> Option<usize> {
        let mir_place = b.borrowed_place.as_ref()?;
        if b.region.0 != r.assigned_at {
            return None;
        }
        let lender_name = r.lender.real_name();
        // MIR's `borrowed_place().local_or_deref_local()` only gives
        // back the base Local for a place — for `&r.a.b` it returns
        // the name `r`, not `r.a.b`. Our lender name is the full
        // dotted path (`r.a.b`). Accept a match when MIR's name is
        // either exactly equal OR is a path prefix terminated by `.`
        // (so `r` matches `r.a.b` but not `roar.a.b`). Without this
        // any borrow rooted at a struct field skips MIR refinement
        // and the lifetime stays pinned to the assignment line —
        // which makes the dotted ref-line render with zero height
        // when the only use of the borrower is inside a macro
        // (where visit_expr can't reach the Path to extend it
        // through update_rap).
        if *mir_place == lender_name {
            return Some(b.region.1);
        }
        if lender_name.starts_with(mir_place)
            && lender_name.as_bytes().get(mir_place.len()) == Some(&b'.')
        {
            return Some(b.region.1);
        }
        None
    }

}