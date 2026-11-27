struct Pair { a: String, b: String }
fn inspect(_: &Pair) {}
pub fn stacked(mut p: Pair) {
    let q = &p; inspect(q); let r = &mut p.a;
    r.clear();
    inspect(&p);
}

pub fn stacked_ends_on_row(mut p: Pair) {
    let q = &p; inspect(q); let r = &mut p.a; r.clear();
    inspect(&p);
}
