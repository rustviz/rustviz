fn main() {
  let mut x = String::from("Hello");
  let y = &mut x; 
  let z = &mut x; // ERROR: y is still live
  String::push_str(y, ", world");
  String::push_str(z, ", friend");
  println!("{}", x);
}