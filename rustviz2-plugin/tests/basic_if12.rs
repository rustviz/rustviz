fn main() {
    let mut x = 8;
    let mut y = 8;
    let mut z = 8;
    let mut s = 9;
    let mut c = &x; // c -> x
    if true {
        c = &y; // c -> y {x} // (returns x)
        x += 8;
        if true {
            c = &z; // c -> z {y, x} (returns y)
            y += 8;
        }
  
        c = &s; // c -> s (returns y)
        y += 9;
    }
  
    println!("c {}", *c);
  }