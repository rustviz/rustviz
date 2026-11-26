fn main() {
    let s = String::from("s");
    let owned = String::new();
    let c = std::env::args().count() > 1;
    let f = || { drop(owned); s.len() };
    if c {
        let g = f;
        g();
    } else {
        println!("{s}");
        f();
    }
}
