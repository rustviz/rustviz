fn main() {
    let s = String::from("s");
    let owned = String::new();
    let c = true;
    let f = || { drop(owned); s.len() };
    if c {
        let g = f;
        g();
    } else {
        println!("{s}");
        f();
    }
}
