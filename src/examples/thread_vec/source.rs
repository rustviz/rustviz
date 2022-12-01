use std::thread; 
fn parent() { 
    let mut v = vec![1 , 2 , 3]; 
    let handle = thread::spawn( move || { 
        v.push(4); // OK , the thread now owns v due to move keyword 
    }); 
    handle.join().unwrap();
} 
