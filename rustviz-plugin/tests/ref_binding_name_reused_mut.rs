fn main() {
    let mut t = (String::from("a"), String::from("b"));
    match t {
        (ref x, _) => {
            println!("{}", x);
        }
    }
    match t {
        (ref mut x, _) => {
            x.push_str("?");
        }
    }
}
