struct Pair { a: String, b: String }
fn run(mut p: Pair, c: bool) {
    if c {
        match p {
            Pair { a: ref a, .. } => {
                let m2 = &mut p.a;
                drop(p.b);
            }
        }
    }
}
