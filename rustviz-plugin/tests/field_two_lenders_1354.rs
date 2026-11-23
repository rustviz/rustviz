// #1354 (open): both fields of `Two<'a>` may borrow from s1 or s2, so the
// call draws four borrows and the println! their four returns, a row each.
// Every pair of them overlaps somewhere between s2 and d.a, and there are
// only three free tracks per row without making the figure taller, so
// two of the arrows still run along one stretch. Pinned as known-wrong
// in `KNOWN_INVARIANT_FAILURES` (arrows-overlap, heads-or-logos-overlap).
struct Two<'p> { a: &'p str, b: &'p str }

fn two<'a>(c: &'a str, d: &'a str) -> Two<'a> {
    Two { a: c, b: d }
}

fn main() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let d = two(&s1, &s2);
    println!("{} {}", d.a, d.b);
}
