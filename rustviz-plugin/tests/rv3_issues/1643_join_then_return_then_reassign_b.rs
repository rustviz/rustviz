fn main() {
    let (c, d) = (std::env::args().len() > 1, std::env::args().len() > 2);
    let mut s: String;
    if c { s = String::from("first"); }
    if d { return; }
    s = String::from("last");
}
