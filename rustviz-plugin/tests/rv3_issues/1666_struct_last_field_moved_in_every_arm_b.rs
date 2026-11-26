struct P { a: String, b: String }
fn f(mut p: P) {
    let r = &mut p.b;
    drop(p.a);
    r.push('x');
}
