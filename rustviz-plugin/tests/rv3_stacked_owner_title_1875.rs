struct Pair { a: String, b: String }
fn inspect(_: &Pair) {}
pub fn stacked(mut p: Pair) {
    let q = &p; inspect(q); let r = &mut p.a;
    r.clear();
    inspect(&p);
}
