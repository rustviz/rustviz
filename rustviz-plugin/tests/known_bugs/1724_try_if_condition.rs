// Known bug, from the closed 2.x fix PRs (cx/cx-text-a); kept as a RustViz 3 regression case.
// Issue(s): #1724 (see known_bugs/README.md for what main draws wrong).
fn run(c: bool, o: Option<u8>, p: Option<u8>) -> Option<u8> {
    let mut s = String::from("a");
    if c && o? > 0 { s = String::new(); } p?;
    Some(0)
}

fn other_conditions(a: Option<u8>, b: Option<u8>) -> Option<()> {
    let mut matched = String::from("old");
    match a? { 0 => matched = String::new(), _ => {} } b?;
    println!("{matched}");

    let mut bound = String::from("old");
    if let 0 = a? { bound = String::new(); } b?;
    println!("{bound}");

    let mut remaining = String::from("old");
    let 0 = a? else { remaining = String::new(); b?; println!("{remaining}"); return None; };
    println!("{remaining}");
    Some(())
}
