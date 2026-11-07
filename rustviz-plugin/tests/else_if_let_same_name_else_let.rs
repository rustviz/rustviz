fn main() {
    let a: Option<i32> = None;
    let b: Option<i32> = Some(2);
    if let Some(z) = a {
        println!("{}", z);
    } else if let Some(z) = b {
        println!("{}", z);
    } else {
        let z = 3;
        println!("{}", z);
    }
}
