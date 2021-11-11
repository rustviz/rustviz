/* --- BEGIN Variable Definitions ---
Owner x;
Closure child;
Function String::from();
--- END Variable Definitions --- */
use std::thread;

fn main() {
  let x = String::from("abc"); // !{ Move(String::from()->x) }
  let child = thread::spawn(move || { // !{ Move(x->child) }
    println!("{}", x.len()); 
  });
  child.join().expect("The thread being joined has panicked"); 
} // !{ GoOutOfScope(x), GoOutOfScope(child) }