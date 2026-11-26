fn main() {
    let s = String::from("s");
    let h;
    {
        let f = || s.len();
        h = f;
    }
    println!("{s}");
    h();
}
