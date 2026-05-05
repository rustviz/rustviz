fn main () {
  let mut x = 7;
  let y = &x;
  let z = &x;
  let c = *y + *z;
  println!("2x value {}", c);
  println!("x value {}", *y);
}