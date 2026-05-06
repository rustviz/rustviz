fn main() {
    let x = 5;
    let y = &x;
    let c = y;
    let z = *c;
    println!("x's original value {}", z);
  }