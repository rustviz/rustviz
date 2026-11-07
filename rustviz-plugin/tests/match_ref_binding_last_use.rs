fn main() {
    let mut t = (String::from("a"), String::from("b"));
    match t {
        (ref x, _) => {
            println!("{}", x);
            t.0.push_str("!");
        }
    }
}
