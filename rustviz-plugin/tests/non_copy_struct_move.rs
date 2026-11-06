struct P {
    x: i32,
}

fn takes(p: P) -> i32 {
    p.x
}

fn main() {
    let p = P { x: 1 };
    let n = takes(p);
}
