fn main() {
    let owned = String::from("owned");
    let s = String::from("shared");
    let f = || { drop(owned); s.len() };
    let g = f;
    println!("{s}");
    g();
}
