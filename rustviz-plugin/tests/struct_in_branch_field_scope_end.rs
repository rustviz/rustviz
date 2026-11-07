struct P {
    x: i32,
}

struct Q {
    x: i32,
    s: String,
}

fn main() {
    let r = P { x: 5 };
    if r.x > 1 {
        let moved = r;
    }
    let q = Q { x: 1, s: String::new() };
    if q.x > 0 {
        let fresh = Q { x: 7, s: String::from("hi") };
    }
}
