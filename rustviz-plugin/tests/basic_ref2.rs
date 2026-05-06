fn main() {
  let x = 7;
  let y = &x;
  let z = &x;
  println!("x {} x {}", *z, *y);
}