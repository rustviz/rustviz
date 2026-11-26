struct D<'a>(&'a mut i32);
impl Drop for D<'_> {
    fn drop(&mut self) { *self.0 += 1; }
}
fn main() {
    let mut n = 0;
    let d = D(&mut n);
    if std::env::args().len() < 2 {
        println!("leaving");
        return;
    }
    drop(d);
}
