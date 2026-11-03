fn double(s: &str) -> Result<i32, std::num::ParseIntError> {
    let n = s.parse::<i32>()?;
    Ok(n * 2)
}

fn main() {
    let r = double("21");
    println!("{:?}", r);
}
