fn main() {
  let x = 5;
  {
    let x = x * 6;
    println!("{}", x);
  }
  println!("{}", x);
}