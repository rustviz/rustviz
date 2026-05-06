fn main() {
  let x = 7;
  let a = &x;
  let b = &a;
  let c = &b;
  println!("x! {} {} {}", *a, **b, ***c);
}