// Assigning to a plain tuple's fields (`t.0 = s`, `t.1 = 5`, `t.1 += 1`).
// Must render: this panicked in resource_of_lhs (#235) until #150 made
// unmodelled LHS places return None and #141 made the colorizer skip
// `.0` tokens. Known gap, not pinned as correct: plain tuples have no
// per-field columns (#189), so the writes themselves aren't drawn. What
// is pinned: `s` is moved into `t.0`, so `s` has nothing left to drop.

fn main() {
    let s = String::from("a");
    let mut t = (String::new(), 1);
    t.0 = s;
    t.1 = 5;
    t.1 += 1;
    println!("{:?}", t);
}
