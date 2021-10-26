/* --- BEGIN Variable Definitions ---
Owner x;
Owner |x|;
Function String::from();
--- END Variable Definitions --- */
use std::thread;

fn main() {
  let x = String::from("abc"); // !{ Move(String::from()->x) }
  let child = thread::spawn(move || { // !{ Move(x->|x|) }
    println!("{}", x.len()); 
  }); // !{ GoOutOfScope(|x|) }
  child.join().expect("The thread being joined has panicked"); 
} // !{ GoOutOfScope(x) }