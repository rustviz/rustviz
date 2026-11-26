struct I { a: String, b: String }
struct P { i: I, z: String }
fn run(p: P, d: bool) {
    if d { drop(p.i.a); } else { drop(p.i.b); }
    let n = 1;
}
