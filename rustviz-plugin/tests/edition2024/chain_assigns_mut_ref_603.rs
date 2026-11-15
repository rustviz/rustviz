fn main() {
    let mut s = String::from("hello");
    let mut t = String::from("hi");
    let y = Some(1);
    let mut r = &mut t;
    if let Some(_n) = y && { r = &mut s; r.len() > 3 } {
        r.push('!');
    } else {
        r.push('?');
    }
    println!("{} {}", s, t);
}
