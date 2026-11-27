// The SVG cannot distinguish two releases of the same loan by their MIR
// successor. Inspect the actual placed events before they become column dots.
use super::*;
use rustc_hir::intravisit::Visitor;

struct CheckBoundaries {
  path: &'static str,
  body: &'static str,
  holder: &'static str,
  lender: &'static str,
  // Fact row, drawn row, exit block, normal successor block.
  edges: &'static [(usize, usize, usize, usize)],
}

impl rustc_driver::Callbacks for CheckBoundaries {
  fn config(&mut self, config: &mut rustc_interface::Config) {
    config.override_queries = Some(rustc_utils::mir::borrowck_facts::override_queries);
  }

  fn after_analysis<'tcx>(&mut self, _: &rustc_interface::interface::Compiler, tcx: rustc_middle::ty::TyCtxt<'tcx>)
    -> rustc_driver::Compilation {
    let facts = crate::facts::collect_crate(tcx, &|_| true);
    let fact = facts.bodies.iter().find(|b| b.func == self.body).unwrap();
    let id = fact.def_id.expect_local();
    let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, id);
    let hir = tcx.hir_body_owned_by(id);
    let source: BTreeMap<_, _> = std::fs::read_to_string(self.path).unwrap().lines()
      .enumerate().map(|(i, l)| (i + 1, l.to_owned())).collect();
    let mut annotated = source.iter().map(|(&i, l)| (i, vec![l.clone()])).collect();
    let mut lines = source.keys().map(|&i| (i, Vec::new())).collect();
    let (mut events, mut owners, mut next, skip) = (Vec::new(), Default::default(), 0, Default::default());
    let mut visitor = ExprVisitor::new(tcx, &bwf.body, hir, bwf, Some(fact), 1, &mut lines, &mut events,
      1, &source, &mut annotated, &mut owners, &mut next, false, &skip);
    visitor.visit_body(hir);
    visitor.emit_borrow_ends();

    let switches = visitor.branch_switches();
    let mut slots = Vec::new();
    templates_in(visitor.preprocessed_events, &mut Vec::new(), None, &mut slots);
    for &(fact_row, row, bb, successor) in self.edges {
      let edge = fact.rows.iter().find(|r| r.line == fact_row).unwrap().facts.iter().find(|e|
        matches!(&e.fact, Fact::BorrowEnd { holder, lender, .. } if holder == self.holder && lender == self.lender)).unwrap();
      let Fact::BorrowEnd { loan, .. } = edge.fact else { unreachable!() };
      assert_eq!(edge.occurrence.at.block.index(), bb);
      assert_eq!(edge.occurrence.successor.unwrap().block.index(), successor);
      let (_, event, path) = slots.iter().find(|(line, e, _)|
        *line == row && endpoints(e).0.real_name() == self.holder && endpoints(e).1.real_name() == self.lender).unwrap();
      let event_id = event.get_id();
      let key = (loan, edge.occurrence);
      let ends = visitor.drawn_references.ends.get(&event_id).expect("placed release lost its MIR edge");
      assert_eq!(ends.iter().map(|e| e.key()).collect::<Vec<_>>(), [key]);
      assert!(visitor.drawn_ends(&switches).edges.contains(&key));
      // Put the recorded event on its sibling, then on the wrong row.
      // Its loan and instruction still match, but it no longer covers this edge.
      let mut sibling = path.clone();
      sibling.last_mut().unwrap().1 ^= 1;
      for (wrong_path, wrong_row) in [(sibling.as_slice(), row), (path.as_slice(), row + 1)] {
        let remove = BTreeSet::from([event_id]);
        remove_from(visitor.preprocessed_events, visitor.event_line_map, &remove);
        insert(visitor.preprocessed_events, visitor.event_line_map, wrong_path, None, (wrong_row, event.clone()), false);
        assert!(!visitor.drawn_ends(&switches).edges.contains(&key));
      }
    }
    rustc_driver::Compilation::Stop
  }
}

fn check(mut case: CheckBoundaries) {
  let sysroot = std::process::Command::new("rustc").args(["--print", "sysroot"]).output().unwrap();
  assert!(sysroot.status.success());
  let sysroot = std::str::from_utf8(&sysroot.stdout).unwrap().trim();
  let args = ["rustc", case.path, "--crate-type=lib", "--edition=2024", "--sysroot", sysroot, "-Awarnings",
    "-Zidentify-regions", "-Zmir-opt-level=0", "-Zmaximal-hir-to-mir-coverage"];
  rustc_driver::run_compiler(&args.map(String::from), &mut case);
}

#[test]
fn failed_pattern_ends_cover_only_their_own_row_and_successor() {
  // The failed let-else uses p on either branch. Success uses x on a third edge.
  check(CheckBoundaries {
    path: concat!(env!("CARGO_MANIFEST_DIR"), "/tests/rv3_let_else_nested_ends.rs"),
    body: "example", holder: "p", lender: "v", edges: &[(5, 5, 6, 7), (9, 9, 13, 14)],
  });
}

#[test]
fn split_nested_ends_keep_their_own_row_and_successor() {
  // The nested if either prints x or goes directly into its empty else.
  check(CheckBoundaries {
    path: concat!(env!("CARGO_MANIFEST_DIR"), "/tests/match_binding_end_in_nested_arm_1513.rs"),
    body: "binding_else", holder: "x", lender: "t", edges: &[(20, 20, 8, 9), (19, 21, 5, 10)],
  });
}
