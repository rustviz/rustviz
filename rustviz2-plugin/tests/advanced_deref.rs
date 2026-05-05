fn main () {
  let mut x = 7;
  let mut c = 8;
  let mut y = &x;
  let z = & mut y;
  *z = &c;
  println!("c value {}", **z);
  y = &x;
  println!("x value {}", *y);
}