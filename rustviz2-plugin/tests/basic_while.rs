fn main () {
  let mut x = 8;
  let mut c = &x;
  let y = 7;
  while x < 10 {
      c = &y;
      x += 1;
      while true {
          x += 1;
      }
  }
}