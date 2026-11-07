fn main() {
    let t = (1, 2);
    match t {
        (x, 5) | (5, x) => println!("{}", x),
        _ => println!("none"),
    }
}
