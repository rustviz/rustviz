fn main() {
    let a = Some(String::from("a"));
    let c = true;
    match a {
        Some(x) => if c {
            println!("{}", x);
        } else {
            drop(x);
        },
        None => {}
    }
}
