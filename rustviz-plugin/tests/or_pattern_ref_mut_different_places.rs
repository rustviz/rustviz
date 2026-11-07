fn main() {
    let mut t = (String::from("a"), String::from("b"));
    match t {
        (ref mut x, _) | (_, ref mut x) => {
            x.push_str("!");
            println!("{}", t.0);
        }
    }
}
