// #1172: `std::process::exit` (and `abort`) end the process without
// running any destructor. What is in scope there is never dropped: its
// column ends at the call, in a dot that says so, and the closing `}` the
// code never reaches drops nothing on that path.
use std::process;

fn run(name: String) -> Result<(), String> {
    if name.is_empty() {
        return Err(String::from("empty"));
    }
    Ok(())
}

// The Book's minigrep (ch. 12).
fn minigrep() {
    let name = String::from("ann");
    let log = String::from("log");
    if let Err(e) = run(name) {
        println!("Application error: {e}");
        process::exit(1);
    }
    println!("{}", log);
}

// A local of the branch, and the function's own value.
fn branch_local(s: String) {
    if s.len() > 5 {
        let t = String::from("b");
        println!("{}", t);
        std::process::exit(1);
    }
    println!("{}", s);
}

// Directly in the function's body.
fn body_level() {
    let u = String::from("a");
    println!("{}", u);
    std::process::exit(0);
}

// A match arm, a let-else's `else`, and `abort` in a loop.
fn arm(code: Result<String, String>) {
    let tag = String::from("tag");
    match code {
        Ok(v) => println!("{} {}", v, tag),
        Err(m) => {
            eprintln!("{}", m);
            process::exit(2)
        }
    }
}

fn let_else(o: Option<String>) {
    let w = String::from("w");
    let Some(x) = o else {
        process::exit(3);
    };
    println!("{} {}", x, w);
}

fn in_loop(v: Vec<String>) {
    for y in v {
        if y.is_empty() {
            process::abort();
        }
        println!("{}", y);
    }
}

fn main() {
    minigrep();
    branch_local(String::from("a"));
    arm(Ok(String::from("ok")));
    let_else(Some(String::from("x")));
    in_loop(vec![String::from("y")]);
    body_level();
}
