/* --- BEGIN Variable Definitions ---
Owner tx; Owner rx; Owner val; Owner received;
Function mpsc::channel(); Function recv(); Function String::from(); Function println!(); Function send();
Function thread::spawn();
--- END Variable Definitions --- */
use std::sync::mpsc;
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel(); // !{ Move(mpsc::channel()->tx),Move(mpsc::channel()->rx)}

    thread::spawn(move || { //!{ MoveToClosure(tx->thread::spawn()) }
        let val = String::from("hello world"); //!{ Move(String::from()->val) }
        tx.send(val).unwrap(); //!{Move(val->send())}
    }); //!{GoOutOfScope(tx),GoOutOfScope(val)}

    let received = rx.recv().unwrap(); //!{Move(recv()->received) }
    println!("Got: {}", received);

}//!{GoOutOfScope(rx), GoOutOfScope(received)}