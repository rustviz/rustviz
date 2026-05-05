//! Ideally we would like to do all of our analysis in the MIR since it is a lot simpler than the HIR
//! since events like drops, borrows and moves are stated explicitly. Additionally, the MIR is less complicated.
//! If you'd like to work on converting more and more of the RustViz logic from the HIR to the MIR I would recommend reading:
//! https://rustc-dev-guide.rust-lang.org/ especially the sections on the MIR, dataflow analysis, borrow checker, drop elaboration
//! Also read through this: https://github.com/rust-lang/polonius/blob/master/book/src/SUMMARY.md
//! Unfortunately though, I don't believe it would be possible to do everything in the MIR, for example: how would 
//! you figure out when a copy occurs? You can't use the assignment operator - since a single HIR instr can be compiled down into
//! multiple MIR instructions.

use crate::{expr_visitor::*, expr_visitor_utils::span_to_line};
use log::info;
use rustc_middle::mir::{Location, Local};
use std::collections::HashMap;
use rustc_utils::mir::{body::BodyExt, place::PlaceExt};
use rustc_utils::SpanExt;
use rustc_borrowck::consumers::{BodyWithBorrowckFacts, RustcFacts};
use rustc_mir_dataflow::move_paths::MoveData;
use rustc_span::Span;
use polonius_engine::{Algorithm, Output, FactTypes};
type Loan = <RustcFacts as FactTypes>::Loan;
type Point = <RustcFacts as FactTypes>::Point;

pub struct MIRBorrowData {
    pub borrow_idx: usize,
    pub region: (usize, usize),
    pub borrowed_place: Option<String>,
    pub assigned_place: Option<String>,
    // could add more useful members if necessary like kind of mutable borrow, regionvid, etc...

}


impl <'a, 'tcx> ExprVisitor<'a, 'tcx> {
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
        let mut loc_to_source_name: HashMap<Local, String> = HashMap::new();
        body_with_facts.body.debug_info_name_map().into_iter().for_each(|(k, v)| {loc_to_source_name.insert(v, k);});
        
        
        // compute the loan regions
        let mut loan_regions: HashMap<Loan, (usize, usize)> = HashMap::new();
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
        
        let mut res: Vec<MIRBorrowData> = Vec::new();

        // refine loan regions (when testing they seem to be off sometimes) by looping through loan assignments
        for (_region, b_idx, location_idx) in body_with_facts.input_facts.as_ref().unwrap().loan_issued_at.iter() {
            let location = body_with_facts.location_table.as_ref().unwrap().to_location(*location_idx);
            let assignment_line = self.location_to_line(location, &body_with_facts);
            match body_with_facts.borrow_set.location_map().get(&location) {
              Some(b_data) => {
                // println!("borrow_data for location {:#?} : {:#?}", location, b_data);
                let b_place = b_data.borrowed_place().local_or_deref_local();
                let a_place = b_data.assigned_place().local_or_deref_local();
                // println!("borrowed_place: {:#?}, as local: {:#?}", b_data.borrowed_place().to_string(self.tcx, &borrow_data.body), b_place);
                // println!("assigned place {:#?}, as local {:#?}", b_data.assigned_place().to_string(self.tcx, &borrow_data.body), a_place);
                let b_name = self.src_name_of_local(b_place, &loc_to_source_name);
                let a_name = self.src_name_of_local(a_place, &loc_to_source_name);

                res.push(MIRBorrowData {borrow_idx: b_idx.index(), 
                                        region: (assignment_line, loan_regions.get(b_idx).unwrap().1),
                                        borrowed_place: b_name, 
                                        assigned_place: a_name});
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
                    info!("borrowed_place: {:#?}, as local: {:#?}", b_data.borrowed_place().to_string(self.tcx, &body_with_facts.body), b_place);
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
                    info!("assigned place {:#?}, as local {:#?}", b_data.assigned_place().to_string(self.tcx, &body_with_facts.body), a_place);
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


    // would be nice to use MIR to figure out where drops occur rather than our own internal state logic
    pub fn gather_drop_data(&self, body_with_facts: &BodyWithBorrowckFacts) -> HashMap<String, usize> {
        // compute local to name map
        let mut loc_to_source_name: HashMap<Local, String> = HashMap::new();
        body_with_facts.body.debug_info_name_map().into_iter().for_each(|(k, v)| {loc_to_source_name.insert(v, k);});

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


    // Would also be nice to use MoveData, https://doc.rust-lang.org/nightly/nightly-rustc/rustc_mir_dataflow/move_paths/struct.MoveData.html
    // To figure out where moves occur and where variables are initialized. Currently we can't handle L-values that are not initialized immediately
    // ex: 
    // let a: Vec<i32>;
    // a = vec![9, 9];
    // Additionally we would need to use this for better granularity of moves
    // ex: 
    // let a = (String::new(), String::new());
    // let b = a.0; (only a.0 is moved)
    // let c = a.1 (a.1 is moved)
    // https://rustc-dev-guide.rust-lang.org/borrow_check/moves_and_initialization/move_paths.html
    #[allow(dead_code)]
    fn gather_move_data(&self, _body: &'tcx rustc_hir::Body, body_with_facts: &BodyWithBorrowckFacts<'tcx>) -> MoveData<'tcx> {
        // The filter decides which types are tracked; `|_| true` tracks all.
        MoveData::gather_moves(&body_with_facts.body, self.tcx, |_| true)
    }

    // Helper function to help refine loan regions that we compute in HIR
    pub fn borrow_match(r: &RefData, b: &MIRBorrowData) -> Option<usize> {
        if b.borrowed_place.is_some() {
            // For now I think it's enough to just check the assignment line and borrowed place for equality
            // Although this is by no means sound logic
            if *b.borrowed_place.as_ref().unwrap() == r.lender.real_name() && b.region.0 == r.assigned_at {
                return Some(b.region.1)
            } 
            else { 
                return None 
            }
        }
        None
    }

}