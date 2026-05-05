fn main () {
  let mut y = 7;
  let mut c = 5;
  let mut x = &c;
  if true {
      
      x = &y;
      c += 7;
  }
  // c += 7;


  println!(" x {}", x);
}