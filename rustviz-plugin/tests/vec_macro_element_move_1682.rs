fn main() {
    let r = String::from("r");
    let s2 = String::from("ab");
    let v = vec![s2, format!("{}!", r)];
    println!("{}", v.len());
}

fn assignment() {
    let prefix = String::from("r");
    let moved = String::from("ab");
    let v;
    v = vec![format!("{}!", prefix), moved];
    println!("{}", v.len());
}

fn argument() {
    let moved_arg = String::from("ab");
    consume(vec![moved_arg, format!("ab")]);
}

fn consume(v: Vec<String>) {
    println!("{}", v.len());
}

// Evaluation stops at the panic: panic_s is never moved into the Vec.
fn after_panic(c: bool) {
    let panic_s = String::from("s");
    if c {
        let pv = vec![panic!("stop"), panic_s];
        println!("{}", pv.len());
    }
}
