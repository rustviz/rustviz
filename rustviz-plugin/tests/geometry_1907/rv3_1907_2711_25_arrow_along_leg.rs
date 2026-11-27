struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn make_pair() -> Pair {
    Pair { a: make(), b: make() }
}

fn split(s: String) -> (String, i32) {
    (s, 1)
}

fn main() {
    let s = String::from("rust");
    let tp = split(s.to_uppercase());
    let p = make_pair();
    match &tp {
        (x2, _) => {
            if s.len() > 1 {
                let (s2, n) = tp;
            } else if s.len() > 1 {
                let n2 = x2.len() as i32;
            } else {
                let Pair { a: r, .. } = &p;
            }
        }
    }
}
