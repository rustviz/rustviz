fn main() {
  let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="9">String::from</tspan>("hello");
  {
      let <tspan data-hash="2">x</tspan> = <tspan class="fn" data-hash="0" hash="9">String::from</tspan>("world");
      println!("{}", <tspan data-hash="2">x</tspan>);
  }
  println!("{}", <tspan data-hash="1">x</tspan>);
}