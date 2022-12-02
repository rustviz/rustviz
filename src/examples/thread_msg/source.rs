use std::sync::mpsc;
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel(); 

    thread::spawn(move || { 
        let val = String::from("hello world"); 
        tx.send(val).unwrap();
    }); 

    let received = rx.recv().unwrap(); 
    println!("Got: {}", received);

}