// Known bug, from the closed 2.x fix PRs (cx/cx-text-a); kept as a RustViz 3 regression case.
// Issue(s): #1724, #1653 (see known_bugs/README.md for what main draws wrong).
fn try_in_let_chain(c: bool, a: Option<u8>, b: Option<u8>) -> Option<()> {
    let mut chained = String::from("old");
    if let 0 = a? && c { chained = String::new(); } b?;
    println!("{chained}");
    Some(())
}

fn after_let_chain(enabled: bool) {
    let chain_pair = (String::from("left"), String::from("right"), false);
    if let (x, _, false) | (_, x, false) = chain_pair && enabled {
        println!("{x}");
    }
}
