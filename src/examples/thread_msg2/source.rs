use std::sync::mpsc;
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel();

    let tx1 = tx.clone();
    thread::spawn(move || {
        let val = String::from("hello world 1");
        tx.send(val).unwrap();
    });

    thread::spawn(move || {
        let val = String::from("hello world 2");
        tx.send(val).unwrap();
    });

    for received in rx {
        println!("Got: {}", received);
    }
}
