fn main() {
    let opt: Option<i32> = Some(3);
    if let Some(x) = opt {
        println!("{}", x);
    }
}
