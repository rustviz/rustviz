use std::thread;

fn main() {
  let <tspan data-hash="1">x</tspan> = <tspan class="fn" data-hash="0" hash="4">String::from</tspan>("abc");
  let <tspan data-hash="2">child</tspan> = <tspan class="fn" data-hash="0" hash="5">thread::spawn</tspan>(move || {
    println!("{}", <tspan data-hash="1">x</tspan>.len());
  });
  <tspan data-hash="2">child</tspan>.join().expect("The thread being joined has panicked");
}