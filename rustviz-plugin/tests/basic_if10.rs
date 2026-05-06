fn main () {
    let mut x = 5;

    if x > 4 {
      if x < 3 {
        x += 6;
      }
      else {
        x += 7;
      }
      if x > 8 {
        x += 8;
      }
    }
    println!("x {}", x);
}
