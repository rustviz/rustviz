fn main () {
    let outer: String = {
      let inner = {
        " world"
      };
      let mut z = String::from("hello ");
      z.push_str(inner);
      z
    };
    println!("outer {}", outer);
  }