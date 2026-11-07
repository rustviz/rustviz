fn main() {
    let mut t = (String::from("a"), String::from("b"));
    match t {
        (ref x, _) | (_, ref x) => {
            println!("{}", x);
            t.1.push_str("!");
        }
    }
}
