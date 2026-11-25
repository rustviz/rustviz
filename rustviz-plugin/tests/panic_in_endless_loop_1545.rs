// #1545: a panic every path gets to (directly in the function's body, in
// a `loop` that never ends, in a labeled block nothing breaks out of)
// unwinds out of the function: what it owns is dropped there, and nothing
// after it runs.
#![allow(unreachable_code)]
fn first() {
    let s = String::from("hey");
    loop {
        panic!("x");
    }
}

fn second() {
    let s2 = String::from("hey");
    let r = &s2;
    loop {
        panic!("x");
        break;
    }
    let r2 = r;
}

fn third() {
    let s3 = String::from("hey");
    loop {
        let t3 = String::from("in");
        let n = t3.len() + s3.len();
        panic!("{}", n);
    }
}

fn fourth() {
    let s4 = String::from("hey");
    loop {
        'b: {
            let t4 = String::from("in");
            panic!("x");
        }
    }
}

fn fifth() {
    let s5 = String::from("hey");
    'b: {
        let t5 = String::from("in");
        panic!("x");
    }
}

fn main() {
    first();
}
