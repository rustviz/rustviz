// Known bug, from the closed 2.x fix PRs (cx/cx-text-b); kept as a RustViz 3 regression case.
// Issue(s): #1666 (see known_bugs/README.md for what main draws wrong).
struct P { a: String, b: String }
struct O { x: String, p: P }

fn reads(o: O, c: bool, d: bool, e: bool) {
    if c { drop(o.x); }
    if d { drop(o.p.a); }
    if e {
        println!("{}", o.p.b);
        drop(o.p.b);
    } else {
        println!("{}", o.p.b);
        drop(o.p.b);
    }
}

fn tail(o: O, c: bool, d: bool) -> String {
    if c { drop(o.x); }
    drop(o.p.a);
    match d {
        true => o.p.b,
        false => o.p.b,
    }
}

fn borrowed(mut p: P) {
    let r = &mut p.b;
    drop(p.a);
    r.push('!');
}
