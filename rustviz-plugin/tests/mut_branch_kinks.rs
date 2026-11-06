// `let mut s` with an if/else and a three-arm match: the thick solid
// branch columns bend at every split and join (#487).

fn main() {
    let mut s = String::from("hi");
    let c = true;
    if c {
        s.push_str("a");
    } else {
        s.push_str("b");
    }
    let n = 3;
    match n {
        1 => s.push_str("x"),
        2 => s.push_str("y"),
        _ => s.push_str("z"),
    }
    println!("{}", s);
}
