// A one-line `else if` (or a `match` in an `else`) whose arms get rows
// of their own (#1264): the inner conditional's fork sits half a row
// above the `else` arm's first row, and the outer `else` leg ends at
// that fork. It ran on to the row, past the fork dot and between the
// inner arms' legs, like a third arm.
fn chain3(c: bool, d: bool, f: String, g: String, h: String) {
    let a = if c { f } else if d { g } else { h };
    println!("{}", a);
}

fn chain4(c: bool, d: bool, e: bool, f: String, g: String, h: String, k: String) {
    let a = if c { f } else if d { g } else if e { h } else { k };
    println!("{}", a);
}

fn chain_borrows(c: bool, d: bool, f: String, g: String, h: String) {
    let r = if c { &f } else if d { &g } else { &h };
    println!("{}", r);
}

fn chain_mut_borrows(c: bool, d: bool, mut f: String, mut g: String, mut h: String) {
    let r = if c { &mut f } else if d { &mut g } else { &mut h };
    r.push('!');
}

fn chain_copies(c: bool, d: bool, x: i32, y: i32, z: i32) {
    let a = if c { x } else if d { y } else { z };
    println!("{} {} {} {}", a, x, y, z);
}

fn chain_assign(c: bool, d: bool, f: String, g: String, h: String) {
    let mut a = String::new();
    println!("{}", a);
    a = if c { f } else if d { g } else { h };
    println!("{}", a);
}

fn match_in_else(c: bool, n: u8, f: String, g: String, h: String) {
    let a = if c { f } else { match n { 0 => g, _ => h } };
    println!("{}", a);
}

fn match_in_match(m: u8, n: u8, f: String, g: String, h: String) {
    let a = match m { 0 => f, _ => match n { 0 => g, _ => h } };
    println!("{}", a);
}

fn if_let_in_else(c: bool, o: Option<String>, f: String, h: String) {
    let a = if c { f } else if let Some(g) = o { g } else { h };
    println!("{}", a);
}

fn in_loop(v: Vec<(String, String, String)>, c: bool, d: bool) {
    for (f, g, h) in v {
        let a = if c { f } else if d { g } else { h };
        println!("{}", a);
    }
}

// No overlap: each arm moves into a variable of its own, further right.
fn chain_apart(c: bool, d: bool, f: String, mut a: String, g: String, mut b: String, h: String, mut e: String) {
    println!("{} {} {}", a, b, e);
    if c { a = f } else if d { b = g } else { e = h }
    println!("{} {} {}", a, b, e);
}

fn main() {
    chain3(true, false, String::new(), String::new(), String::new());
    chain4(true, false, true, String::new(), String::new(), String::new(), String::new());
    chain_borrows(true, false, String::new(), String::new(), String::new());
    chain_mut_borrows(true, false, String::new(), String::new(), String::new());
    chain_copies(true, false, 1, 2, 3);
    chain_assign(true, false, String::new(), String::new(), String::new());
    match_in_else(true, 0, String::new(), String::new(), String::new());
    match_in_match(0, 0, String::new(), String::new(), String::new());
    if_let_in_else(true, None, String::new(), String::new());
    in_loop(Vec::new(), true, false);
    chain_apart(true, false, String::new(), String::new(), String::new(), String::new(), String::new(), String::new());
}
