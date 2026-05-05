fn main () {
  let mut x = 7;
  let mut z = 6;
  let mut a = & mut x;
  let mut c = & mut z;
  let mut b = & mut a;
  b = & mut c;
  println!("x {}", *a);
  println!("z {}", **b);
}