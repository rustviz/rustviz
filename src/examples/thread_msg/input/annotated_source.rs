use std::sync::mpsc;
use std::thread;

fn main() {
    let (<tspan data-hash="1">tx</tspan>, <tspan data-hash="2">rx</tspan>) = <tspan class="fn" data-hash="0" hash="5">mpsc::channel</tspan>(); 

    thread::spawn(move || { 
        let <tspan data-hash="3">val</tspan> = String::from("hello world"); 
        <tspan data-hash="1">tx</tspan>.send(<tspan data-hash="3">val</tspan>).unwrap();
    }); 

    let <tspan data-hash="4">received</tspan> = <tspan data-hash="2">rx</tspan>.<tspan class="fn" data-hash="0" hash="6">recv</tspan>().unwrap(); 
    println!("Got: {}", <tspan data-hash="4">received</tspan>);

}