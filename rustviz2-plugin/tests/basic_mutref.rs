fn main() {
  let mut x = 7;
  let y = & mut x;
  *y += 3;
  println!("x {}", *y);
}