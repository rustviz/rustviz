fn main() {
    let c = std::env::args().len() > 1;
    let mut s = String::from("old");
    if c { drop(s); }
    if c { return; }
    s = String::from("new");
}
