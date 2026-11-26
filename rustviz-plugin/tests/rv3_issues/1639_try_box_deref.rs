fn main() -> Result<(), String> {
    let b = Box::new(Err::<(), _>(String::new()));
    (*b)?;
    Ok(())
}
