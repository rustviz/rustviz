// Several value `break`s (#561): values declared inside the loop (the
// `break` that doesn't carry one drops it, #410), three `match` arms,
// and one variable that every `break` carries (so it is moved, not
// maybe-moved).

fn main() {
    let c = std::env::args().count() > 1;
    let r = loop {
        let k = String::from("k");
        let j = String::from("j");
        if c { break k; } else { break j; }
    };
    let a = String::from("a");
    let n = std::env::args().count();
    let w = loop {
        match n {
            0 => break a,
            1 => break String::from("one"),
            _ => break String::from("many"),
        }
    };
    let b = String::from("b");
    let v = loop {
        if c { break b; } else { break b; }
    };
    println!("{} {} {}", r, w, v);
}
