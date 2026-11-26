// Known bug, from the closed 2.x fix PRs (cx/cx-c18); kept as a RustViz 3 regression case.
// Issue(s): #1750 (see known_bugs/README.md for what main draws wrong).
fn mixed(cond: bool) {
    let a = 1;
    let mut b = 2;
    let h = if cond { Some((&a, &mut b)) } else { None };
    if let Some((_, m)) = h { *m = 5; }
    b = 3;
}

fn assigned(flag: Option<i32>) {
    let c = 1;
    let mut d = 2;
    let mut dst = None;
    dst = match flag {
        Some(_) => Some((&c, &mut d)),
        None => None,
    };
    println!("{c} {dst:?}");
}

fn via_ref(flag: bool) {
    let e = 1;
    let mut f = 2;
    let r = &mut f;
    let pair: Option<(&i32, &mut i32)> = if flag { Some((&e, r)) } else { None };
    println!("{pair:?}");
}
