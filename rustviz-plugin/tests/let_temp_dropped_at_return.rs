// #553: a `let`-extended temporary (#400) that is still alive at an
// early `return` is dropped there, at the `return`. Its drop dot must
// not say it goes out of scope "at the end of the block": that is
// where it is dropped when no `return` leaves first (the `r.len()`
// path below), and the tooltip on that path keeps that wording.
fn f(x: u32) -> usize {
    let r = &String::from("a");
    match x {
        0 => return 0,
        _ => {}
    }
    r.len()
}

fn main() {
    f(1);
}
