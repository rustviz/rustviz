fn main() {
    let c = std::env::args().len() > 1;
    let mut s: String;
    if c { s = String::from("first"); }
    if c { return; }
    s = String::from("last");
}
