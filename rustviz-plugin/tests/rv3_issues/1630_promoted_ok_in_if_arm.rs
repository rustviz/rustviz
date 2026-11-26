fn main() {
    let r: &'static Result<i32, String> = if std::hint::black_box(true) {
        &Ok(1)
    } else {
        &Ok(2)
    };
    println!("{:?}", r);
}
