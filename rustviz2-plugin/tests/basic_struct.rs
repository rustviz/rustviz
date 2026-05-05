pub struct Apple {
  x: u32, 
  y: u32
}


fn main () {
  let z = Apple { x: 8, y: 9};
  let c = &z;
  println!("{}", c.x);
}