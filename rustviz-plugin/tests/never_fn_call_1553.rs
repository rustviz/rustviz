// #1553: a call to a function of the user's that returns `!`. One whose
// body always panics (`fail`) unwinds, dropping what is in scope, as a
// `panic!()` there would; one that always ends the process (`quit`) or
// loops forever (`spin`) drops nothing; for one whose body doesn't say
// (`maybe`: it panics or loops), the tooltip says what happens either way.
// main drew none of these calls as leaving.
fn fail() -> ! {
    panic!("x")
}

fn main() {
    let s = String::from("hey");
    loop {
        fail();
    }
}

fn in_branch() {
    let s1 = String::from("hey");
    if s1.len() > 1 {
        fail();
    }
    println!("{}", s1);
}

fn quit(code: i32) -> ! {
    eprintln!("bye");
    std::process::exit(code)
}

fn exits() {
    let s2 = String::from("hey");
    if s2.len() > 1 {
        quit(1);
    }
    println!("{}", s2);
}

fn spin() -> ! {
    loop {}
}

fn spins() {
    let s3 = String::from("hey");
    if s3.len() > 1 {
        spin();
    }
    println!("{}", s3);
}

fn maybe(n: u32) -> ! {
    if n > 3 { panic!("big") } else { loop {} }
}

fn unknown() {
    let s4 = String::from("hey");
    maybe(2);
}

struct App;

impl App {
    fn die(&self) -> ! {
        unreachable!("done")
    }
}

fn method() {
    let app = App;
    let s5 = String::from("hey");
    match s5.len() {
        0 => app.die(),
        _ => println!("{}", s5),
    }
}

// Sibling contexts: a let-else's `else`, a branch of a `let`'s value, a
// call's argument.
fn let_else(o: Option<u32>) {
    let s6 = String::from("hey");
    let Some(n) = o else { fail() };
    println!("{} {}", s6, n);
}

fn let_value() {
    let s7 = String::from("hey");
    let n: u32 = if s7.is_empty() { spin() } else { 3 };
    println!("{} {}", s7, n);
}

fn argument() {
    let s8 = String::from("hey");
    if s8.len() > 9 {
        drop(fail());
    }
    println!("{}", s8);
}
