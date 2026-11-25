// #1479: a reference re-pointed in some arms of an `else if` chain may
// still hold its first loan after the chain; its last use gives each
// lender back once.

// The issue's program: c may point at x, y or z after the chain.
fn else_if_chain() {
    let x = 1;
    let y = 2;
    let z = 3;
    let n = 2;
    let mut c = &x;
    if n == 0 {
        c = &y;
    } else if n == 1 {
        c = &z;
    } else {
        println!("{}", c);
    }
    println!("{}", c);
}

// The same with `&mut`.
fn else_if_chain_mut() {
    let mut x = String::from("x");
    let mut y = String::from("y");
    let mut z = String::from("z");
    let n = 2;
    let mut c = &mut x;
    if n == 0 {
        c = &mut y;
    } else if n == 1 {
        c = &mut z;
    } else {
        c.push('!');
    }
    c.push('?');
}

// A `match` whose arms re-point it.
fn match_arms() {
    let x = 1;
    let y = 2;
    let z = 3;
    let n = 2;
    let mut c = &x;
    match n {
        0 => c = &y,
        1 => {
            c = &z;
        }
        _ => println!("{}", c),
    }
    println!("{}", c);
}

// A longer chain: two arms leave c pointing at x.
fn longer_chain() {
    let x = 1;
    let y = 2;
    let z = 3;
    let n = 2;
    let mut c = &x;
    if n == 0 {
        c = &y;
    } else if n == 1 {
        c = &z;
    } else if n == 2 {
        println!("{}", c);
    } else {
        println!("{}", n);
    }
    println!("{}", c);
}

fn main() {
    else_if_chain();
    else_if_chain_mut();
    match_arms();
    longer_chain();
}
