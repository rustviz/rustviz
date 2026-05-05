fn main () {
    let mut x = 5;
    let mut y = 7;
  
    if x > 4 {
      x = y;
    }
    else {
      y = x;
    }
    println!("x {}", x);
  }