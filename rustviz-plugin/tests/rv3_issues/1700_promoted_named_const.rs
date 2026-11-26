fn main() {
    const N: i32 = 1;
    let q: &'static Result<i32, String> = &Ok(N);
    println!("{:?}", q);
}
