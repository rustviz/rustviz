fn main() {
  let x = 7;
  let y = &x;
  let z = y;
  println!("x {} x {}", *z, *y);
}