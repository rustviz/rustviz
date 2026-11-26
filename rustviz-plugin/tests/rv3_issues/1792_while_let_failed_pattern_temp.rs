struct Guard;
impl Drop for Guard { fn drop(&mut self) { println!("drop"); } }
fn make() -> Result<u8, Guard> { Err(Guard) }
fn f() {
    while let Ok(ref g) = make() {
        println!("{g}");
    }
}
