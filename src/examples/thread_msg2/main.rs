/* --- BEGIN Variable Definitions ---
Owner tx; Owner rx; Owner tx1; 
Function mpsc::channel(); Function clone(); Function String::from(); Function println!();
Function thread::spawn();
--- END Variable Definitions --- */
use std::sync::mpsc;
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel(); // !{ Move(mpsc::channel()->tx),Move(mpsc::channel()->rx)}

    let tx1 = tx.clone(); //!{Copy(tx->tx1)}
    thread::spawn(move || { //!{ MoveToClosure(tx->thread::spawn()) }
        let val = String::from("hello world 1"); 
        tx.send(val).unwrap();
    });//!{GoOutOfScope(tx)}

    thread::spawn(move || { //!{ MoveToClosure(tx1->thread::spawn()) }
        let val = String::from("hello world 2"); 
        tx.send(val).unwrap();
    });//!{GoOutOfScope(tx1)}

    for received in rx { 
        println!("Got: {}", received);
    }
}// !{GoOutOfScope(rx)}