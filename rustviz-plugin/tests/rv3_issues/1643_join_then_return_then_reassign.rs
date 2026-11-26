fn main() {
    let (c, d) = (std::env::args().len() > 1, std::env::var_os("D").is_some());
    let mut s = String::from("old");
    if c { drop(s); }
    if d { return; }
    s = String::from("new");
}
