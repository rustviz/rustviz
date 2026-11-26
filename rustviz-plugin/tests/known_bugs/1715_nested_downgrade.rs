// Known bug, from the closed 2.x fix PRs (cx/cx-c18); kept as a RustViz 3 regression case.
// Issue(s): #1715 (see known_bugs/README.md for what main draws wrong).
fn downgrade(s: &mut i32) -> &i32 { s }
fn id(s: &i32) -> &i32 { s }

fn nested() {
    let mut a = 1;
    let mut v: Vec<&i32> = Vec::new();
    v.push(id(downgrade(&mut a)));
    println!("{v:?}");
}

fn pass(s: &i32) -> Option<&i32> { Some(s) }

fn through_try() -> Option<()> {
    let mut b = 1;
    let mut out = Vec::new();
    out.push(pass(id(downgrade(&mut b)))?);
    println!("{out:?}");
    Some(())
}

fn in_loop(cond: bool) {
    let mut c = 1;
    let r = downgrade(&mut c);
    let mut items = Vec::new();
    while cond {
        items.push(id(r));
        break;
    }
    println!("{items:?}");
}

fn bound() {
    let mut d = 1;
    let held = Some(id(downgrade(&mut d)));
    let Some(x) = held else { return };
    println!("{x}");
}
