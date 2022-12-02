use std::sync::mpsc;
use std::thread;

fn main() {
    let (<tspan data-hash="1">tx</tspan>, <tspan data-hash="2">rx</tspan>) = <tspan class="fn" data-hash="0" hash="3">mpsc::channel</tspan>();

    let  <tspan data-hash="3">tx1</tspan> = <tspan data-hash="1">tx</tspan>.<tspan class="fn" data-hash="0" hash="4">clone();</tspan>
    <tspan class="fn" data-hash="0" hash="5">thread::spawn</tspan>(move || {
        let val = <tspan class="fn" data-hash="0" hash="7">String::from</tspan>("hello world 1");
        <tspan data-hash="1">tx</tspan>.send(val).unwrap();
    });

    <tspan class="fn" data-hash="0" hash="5">thread::spawn</tspan>(move || {
        let  val = <tspan class="fn" data-hash="0" hash="7">String::from</tspan>("hello world 2");
        <tspan data-hash="3">tx1</tspan>.send( val).unwrap();
    });

    for received in <tspan data-hash="2">rx</tspan> {
        println!("Got: {}", received);
    }
}