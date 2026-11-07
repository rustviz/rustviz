fn main() {
    let v = vec![1, 2, 3];
    let third = v.get(2);
    match third {
        Some(x) => println!("{}", x),
        None => {}
    }
}
