fn parse() -> Result<i32, std::num::ParseIntError> {
    let n = "5".parse::<i32>().map(|x| x + 1)?;
    Ok(n)
}

fn main() {
    let _ = parse();
}
