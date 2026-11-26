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

fn quit() -> ! {
    std::process::exit(0)
}

// Evaluation stops at quit(): quit_s is never moved into the Vec.
fn after_never_call() {
    let quit_s = String::from("s");
    let qv = vec![format!("first"), quit(), quit_s];
    println!("{}", qv.len());
}

fn wrap(_: ()) -> String {
    String::new()
}

// The argument returns before wrap_s is reached; so does the block's
// first statement before block_s.
fn nested_divergence(c: bool) {
    let wrap_s = String::from("s");
    let block_s = String::from("s");
    if c {
        let wv = vec![format!("first"), wrap(return), wrap_s];
        println!("{}", wv.len());
    } else {
        let bv = vec![format!("first"), { quit(); String::new() }, block_s];
        println!("{}", bv.len());
    }
}
