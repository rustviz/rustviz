/* --- BEGIN Variable Definitions ---
Function join();
Function parent();
Function vec!();
Function thread::spawn();
Owner handle;
Owner v;
Function unwrap();
--- END Variable Definitions --- */
use std::thread;
fn parent() {
    static v : [ i32 ; 3] = [1, 2, 3]; // !{ Move(vec!()->v) }
    let handle = thread::spawn(|| { // !{ MoveToClosure(v->thread::spawn()) }
        println!("{}", v[0]); // OK, v guaranteed to outlive thread
    });
    handle.join().unwrap();
}// !{ GoOutOfScope(v) }

fn main() {
    parent();
}