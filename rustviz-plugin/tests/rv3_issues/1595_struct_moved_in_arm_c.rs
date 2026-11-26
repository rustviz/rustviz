struct Pair {
    a: String,
    b: String,
}

fn run(mut p: Pair, c: bool) {
    if c {
        drop(p.a);
        let r = &mut p.b;
        r.push('x');
    }
}
