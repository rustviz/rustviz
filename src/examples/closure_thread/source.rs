use std::thread;

fn main() {
  let x = String::from("abc");
  let child = thread::spawn(move || {
    println!("{}", x.len());
  });
  child.join().expect("The thread being joined has panicked");
}