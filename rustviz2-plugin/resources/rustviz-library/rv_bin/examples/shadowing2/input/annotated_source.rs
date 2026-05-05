
fn main() {
  let <tspan data-hash="1">x</tspan> = 5;
  {
    let <tspan data-hash="1">x</tspan> = <tspan data-hash="1">x</tspan> * 6;
    println!("{}", <tspan data-hash="4">x</tspan>);
  }
  println!("{}", <tspan data-hash="5">x</tspan>);
}