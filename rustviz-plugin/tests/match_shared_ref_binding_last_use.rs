fn main() {
    let mut t = (String::from("a"), String::from("b"));
    let o = &t;
    match o {
        (x, _) => {
            println!("{}", x);
            t.0.push_str("!");
        }
    }
}
