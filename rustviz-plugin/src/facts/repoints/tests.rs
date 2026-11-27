use crate::facts::Fact;

#[test]
fn a_caller_reference_is_a_reaching_definition_at_the_join() {
    struct Check;

    impl rustc_driver::Callbacks for Check {
        fn config(&mut self, config: &mut rustc_interface::Config) {
            config.override_queries = Some(rustc_utils::mir::borrowck_facts::override_queries);
        }

        fn after_analysis<'tcx>(
            &mut self, _: &rustc_interface::interface::Compiler, tcx: rustc_middle::ty::TyCtxt<'tcx>,
        ) -> rustc_driver::Compilation {
            let facts = crate::facts::collect_crate(tcx, &|_| true);
            let facts = facts.bodies.iter().find(|b| b.func == "caller_reaches_join").unwrap();
            let bwf = rustc_utils::mir::borrowck_facts::get_body_with_borrowck_facts(tcx, facts.def_id.expect_local());
            let body = &bwf.body;
            let moves = rustc_mir_dataflow::move_paths::MoveData::gather_moves(body, tcx, |_| true);
            let doms = body.basic_blocks.dominators();
            let runs = body.basic_blocks.iter_enumerated().filter(|(bb, data)| !data.is_cleanup && doms.is_reachable(*bb))
                .map(|(bb, _)| bb).collect();
            let stores = super::IncomingStores::new(tcx, body, &runs, &moves);
            // The true edge stores the reference to pair.0. The false
            // edge reaches the read with the reference from the caller.
            let overwrite = facts.rows.iter().flat_map(|r| &r.facts).filter(|e|
                matches!(&e.fact, Fact::Repoint { holder, .. } if holder == "r")).last().unwrap();
            let at = overwrite.occurrence.at;
            let rustc_middle::mir::StatementKind::Assign(write) = &body[at.block].statements[at.statement_index].kind
                else { panic!("expected the unconditional assignment") };
            let incoming = stores.definitions(at, write.0);
            assert!(incoming.argument && incoming.sites.len() == 1 && !incoming.unknown && !incoming.uninitialized);
            assert_eq!(stores.direct_borrow(at, write.0), None,
                "the explicit assignment is only one of the incoming definitions");
            assert!(stores.direct_borrow(at.successor_within_block(), write.0).is_some(),
                "the unconditional assignment supplies a single definition");
            rustc_driver::Compilation::Stop
        }
    }

    let sysroot = std::process::Command::new("rustc").args(["--print", "sysroot"]).output().unwrap();
    assert!(sysroot.status.success());
    let sysroot = std::str::from_utf8(&sysroot.stdout).unwrap().trim();
    let args = ["rustc", concat!(env!("CARGO_MANIFEST_DIR"), "/tests/p4_direct_holders.rs"),
        "--crate-type=lib", "--edition=2024", "--sysroot", sysroot, "-Awarnings",
        "-Zidentify-regions", "-Zmir-opt-level=0", "-Zmaximal-hir-to-mir-coverage"];
    rustc_driver::run_compiler(&args.map(String::from), &mut Check);
}
