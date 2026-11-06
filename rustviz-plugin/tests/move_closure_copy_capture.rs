// A `move` closure bound with `let` copies what it captures when the
// captured value is Copy (#311): `m` and `p.x` are still usable after
// the closure is made, and the closure has nothing of theirs to drop.
// `s` (a String) is moved in and dropped with `f`. `r` is a `&String`,
// which is Copy: `g` gets a copy of the reference, not of `t`.

struct P {
    name: String,
    x: i32,
}

fn main() {
    let m = 3;
    let s = String::from("hi");
    let p = P { name: String::from("a"), x: 1 };
    let f = move || s.len() as i32 + m + p.x;
    let t = String::from("yo");
    let r = &t;
    let g = move || r.len();
    println!("{} {} {} {} {}", f(), m, p.name, g(), t);
}
