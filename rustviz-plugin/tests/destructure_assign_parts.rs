// More destructuring assignments (#151): a rotation of Strings, a
// part nobody receives (`_`: t is moved into the temporary and
// dropped with it), a swap of structs (their fields go along), and a
// struct pattern.
struct P {
    name: String,
}

struct Pt {
    x: i32,
    y: i32,
}

fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut c = String::from("c");
    (a, b, c) = (b, c, a);
    let s = String::from("s");
    let t = String::from("t");
    (a, _) = (s, t);
    let mut p = P { name: String::from("p") };
    let mut q = P { name: String::from("q") };
    (p, q) = (q, p);
    let mut x = 0;
    let mut y = 0;
    Pt { x, y } = Pt { x: 1, y: 2 };
    println!("{} {} {} {} {} {} {}", a, b, c, p.name, q.name, x, y);
}
