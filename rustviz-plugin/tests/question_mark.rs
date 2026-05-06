fn parse() -> Result<i32, std::num::ParseIntError> {
    let n: i32 = "5".parse()?;
    Ok(n)
}

fn main() {
    let _ = parse();
}
