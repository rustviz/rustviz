fn main () {
  let mut x = 7;
  let mut y = 8;

  let mut a = &x;
  let b = &mut a;

  let mut c = &y;
  let d = &mut c;

  *b = *d;
  println!("y {}", **b);
}