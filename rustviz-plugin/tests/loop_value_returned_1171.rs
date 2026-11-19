// #1171: a `loop` that is the function's value (its tail, or what a
// `return` carries) hands what its `break` carries to the caller. On the
// iteration that breaks, nothing is dropped; the iterations that go round
// again drop the loop's own value at the end of the body.
fn read_line() -> String {
    String::from("ann")
}

fn get_name() -> String {
    loop {
        let line = read_line();
        if !line.is_empty() {
            break line;
        }
    }
}

fn first(s: String) -> String {
    return loop {
        break s;
    };
}

fn tail(a: String) -> String {
    loop {
        break a;
    }
}

fn in_branch(c: bool, b: String) -> String {
    if c {
        return loop {
            break b;
        };
    }
    String::new()
}

fn arm(n: i32, d: String, e: String) -> String {
    match n {
        0 => loop {
            break d;
        },
        _ => e,
    }
}

// Several `break`s: only the one that ended the loop ran.
fn several(c: bool, f: String, g: String) -> String {
    loop {
        if c {
            break f;
        }
        break g;
    }
}

fn labeled(h: String) -> String {
    'outer: loop {
        loop {
            break 'outer h;
        }
    }
}

fn main() {
    println!("{}", get_name());
    println!("{}", first(String::from("a")));
    println!("{}", tail(String::from("b")));
    println!("{}", in_branch(true, String::from("c")));
    println!("{}", arm(0, String::from("d"), String::from("e")));
    println!("{}", several(true, String::from("f"), String::from("g")));
    println!("{}", labeled(String::from("h")));
}
