/* --- BEGIN Variable Definitions ---
Owner x;
Closure ||;
Function String::from();
Function thread::spawn();
--- END Variable Definitions --- */
use std::thread;

fn main() {
  let x = String::from("abc"); // !{ Move(String::from()->x) }
  let child = thread::spawn(move || { // !{ MoveToClosure(x->||), Move(||->thread::spawn()) }
    println!("{}", x.len()); 
  }); //!{ GoOutOfScope(||) }
  child.join().expect("The thread being joined has panicked"); 
} // !{ GoOutOfScope(x) }