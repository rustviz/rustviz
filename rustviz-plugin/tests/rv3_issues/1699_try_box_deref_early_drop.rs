fn run(b: Box<Result<(), String>>) -> Result<(), String> {
    (*b)?;
    Ok(())
}
