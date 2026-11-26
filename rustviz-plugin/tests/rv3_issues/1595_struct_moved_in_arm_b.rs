struct Pair {
    a: String,
    b: String,
}

fn run(p: Pair, c: bool, d: bool) {
    if c {
        drop(p.a);
    }
    if d {
        drop(p.b);
    } else {
        drop(p.b);
    }
}
