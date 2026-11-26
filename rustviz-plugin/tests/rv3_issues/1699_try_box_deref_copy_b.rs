fn run(b: Box<Option<i32>>) -> Option<i32> {
    let x = (*b)?;
    Some(x)
}
