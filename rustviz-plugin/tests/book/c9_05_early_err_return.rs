fn check(x: i32) -> Result<i32, String> {
    if x < 0 {
        return Err(String::from("negative"));
    }
    Ok(x)
}

fn main() {
    let r = check(5);
    println!("{:?}", r);
}
