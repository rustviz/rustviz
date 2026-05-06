fn main() {
    let mut z = String::from("hello ");
    z.push_str(world_slice());
  }
  
  fn world_slice () -> &'static str {
    " world"
  }