fn main() {
    let a: Option<i32> = None;
    let b: Option<i32> = Some(2);
    if let Some(x) = a {
        println!("{}", x);
    } else if let Some(y) = b {
        println!("{}", y);
    }
}
