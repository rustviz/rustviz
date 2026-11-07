fn main() {
    let v = vec![1, 2];
    match v.get(0) {
        Some(x) => println!("{}", x),
        None => {}
    }
}
