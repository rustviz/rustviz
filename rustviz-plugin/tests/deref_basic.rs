fn main () {
    let mut y = 7;
    let x = &mut y;
    *x = 8;
    let z = *x;
  }