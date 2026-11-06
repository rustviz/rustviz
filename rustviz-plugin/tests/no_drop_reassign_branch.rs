struct P {
    x: i32,
}

fn main() {
    let mut p = P { x: 1 };
    p = P { x: 2 };
    let r = P { x: 5 };
    if p.x > 1 {
        let moved = r;
    }
    let q = if p.x > 1 { P { x: 3 } } else { P { x: 4 } };
}
