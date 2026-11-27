fn read(value: &String) { println!("{value}"); }
fn main() {
    let a = String::from("a");
    let mut s = String::from("s");
    let mut r = &a;
    read(r);
    r = &s; read(r); s.push('1'); r = &s; read(r);
    s.push('2');
}

fn alternate(take_left: bool, s: String, t: String) {
    let source = &s;
    let mut r = &t;
    read(r);
    if take_left { r = source; } else { r = source; }
    read(r);
    r = &t;
    read(r);
}
