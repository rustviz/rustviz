// Known bug, from the closed 2.x fix PRs (cx/cx-c18); kept as a RustViz 3 regression case.
// Issue(s): #1664 (see known_bugs/README.md for what main draws wrong).
fn slot_case() {
    let a = String::from("a");
    let mut r = &a;
    let slot: &mut &String = &mut r;
    let mut u: Vec<&mut &String> = Vec::new();
    u.push(slot);
    println!("{a} {u:?}");
}
