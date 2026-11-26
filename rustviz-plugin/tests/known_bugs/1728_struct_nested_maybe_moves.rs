// Known bug, from the closed 2.x fix PRs (cx/cx-text-b); kept as a RustViz 3 regression case.
// Issue(s): #1728 (see known_bugs/README.md for what main draws wrong).
struct I { a: String, b: String }
struct P { i: I, z: String }

fn run(p: P, d: bool) {
    if d { drop(p.i.a); } else { drop(p.i.b); }
    let n = 1;
}

fn both(p: P) {
    let a = p.i.a;
    let b = p.i.b;
    println!("{}", p.z);
}

fn either(p: P, d: bool) {
    if d {
        drop(p.i.a);
        drop(p.i.b);
        drop(p.z);
    }
    let n = 1;
}
