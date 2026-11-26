// Known bug, from the closed 2.x fix PRs (cx/cx-c18); kept as a RustViz 3 regression case.
// Issue(s): #1750 (see known_bugs/README.md for what main draws wrong).
fn chain(cond: Option<bool>) {
    let shared = 1;
    let mut exclusive = 2;
    let result = if let Some(ok) = cond && ok {
        Some((&shared, &mut exclusive))
    } else {
        None
    };
    println!("{shared} {result:?}");
}
