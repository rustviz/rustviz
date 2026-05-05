fn main () {
    let mut x = 5;

    if x > 4 {
      if x < 3 {
        x += 5;
      }
      else {
        x += 6;
      }
    }
    else {
        if x > 3 {
            x += 5;
        }
        else {
            x += 5;
        }
        x += 2;
    }
    println!("x {}", x);
}
