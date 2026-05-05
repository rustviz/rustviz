fn main () {
    let mut x = 5;
    let mut y = 8;

    if x > 4 {
      if x < 3 {
        x = y;
      }
      else {
        y = x;
      }
    }
    else {
        if x > 3 {
            x = y;
        }
        else {
            y = x;
        }
    }
    println!("x {}, y {}", x, y);
}
