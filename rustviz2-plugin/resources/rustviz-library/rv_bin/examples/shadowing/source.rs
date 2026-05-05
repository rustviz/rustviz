fn main() {
  let x = String::from("hello");
  {
    let x = String::from("world");
    println!("{}", x);
  } 
  println!("{}", x);
} 