fn share(r: &mut i32) -> (&i32,) { (r,) }
fn main() { let mut b = 1; let t = share(&mut b); println!("{}", t.0); }
