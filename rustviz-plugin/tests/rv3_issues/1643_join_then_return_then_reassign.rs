fn main() {
    let (c, d) = (std::env::args().len() > 1, std::env::args().len() > 2);
    let mut s = String::from("old");
    if c { drop(s); }
    if d { return; }
    s = String::from("new");
}
