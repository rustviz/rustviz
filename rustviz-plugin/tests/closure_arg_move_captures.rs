// A closure passed straight to a call takes what it captures by value
// with it: `v` and `s` are moved into the closure handed to
// thread::spawn (the spawned thread drops them), `n` is copied, and a
// `move` closure that only uses `p.name` captures just that field (#230).

use std::thread;

struct P {
    name: String,
    age: u32,
}

fn main() {
    let v = vec![1, 2, 3];
    let n = 5;
    let handle = thread::spawn(move || {
        println!("{:?} {}", v, n);
    });
    handle.join().unwrap();

    let s = String::from("s");
    let h2 = thread::spawn(|| drop(s)); // no `move`, but drop(s) needs s by value
    h2.join().unwrap();

    let p = P { name: String::from("p"), age: 3 };
    let h3 = thread::spawn(move || p.name.len());
    h3.join().unwrap();
    println!("{} {}", n, p.age);
}
