/* --- BEGIN Variable Definitions ---
Function unwrap();
Function join();
Function thread::spawn();
Function parent();
Function vec!();
Owner mut v;
Owner handle;
--- END Variable Definitions --- */
use std::thread; 
fn parent() { 
    let mut v = vec![1 , 2 , 3]; // !{ Move(vec!()->v) }
    let handle = thread::spawn( move || { // !{ MoveToClosure(v->thread::spawn()) }
        v.push(4); // OK , the thread now owns v due to move keyword 
    }); 
    handle.join().unwrap();
} // !{ GoOutOfScope(v) }
