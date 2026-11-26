struct Pair { a: String, b: String }

fn run(mut p: Pair, c: bool) {
    drop(p.a);
    if c {
        let k = 1;
        p.a = String::new();
    }
}
