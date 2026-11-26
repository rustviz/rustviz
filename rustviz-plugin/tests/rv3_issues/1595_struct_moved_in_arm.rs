struct Pair {
    a: String,
    b: String,
}

fn run(p: Pair, c: bool) {
    if c {
        let Pair { a, b } = p;
    }
}

fn main() {
    run(Pair { a: String::from("a"), b: String::from("b") }, true);
}
