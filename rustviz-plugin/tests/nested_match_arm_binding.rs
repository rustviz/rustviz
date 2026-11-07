fn main() {
    let a = Some(1);
    let b = Some(2);
    match a {
        Some(x) => match b {
            Some(y) => println!("{}", y),
            None => println!("{}", x),
        },
        None => {}
    }
}
