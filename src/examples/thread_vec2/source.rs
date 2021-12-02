use std::thread;
fn parent() {
    static v : [ i32 ; 3] = [1, 2, 3]; // fixed-length array
    let handle = thread::spawn(|| {
        println!("{}", v[0]); // OK, v guaranteed to outlive thread
    });
    handle.join().unwrap();
}

fn main() {
    parent();
}