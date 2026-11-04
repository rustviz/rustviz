// #395: a struct pattern in one guarded arm moves `p.a` out only when
// that arm is taken; `p.b` stays in `p` on every path and is dropped
// at the end of `main`.

struct P {
    a: String,
    b: String,
}

fn take(s: String) -> usize {
    s.len()
}

fn main() {
    let p = P { a: String::from("a"), b: String::from("b") };
    let c = true;
    match p {
        P { a, .. } if c => {
            take(a);
        }
        _ => {}
    }
}
