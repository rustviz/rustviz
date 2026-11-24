// #1476: a holder of two loans that gives them back on a conditional's
// path draws each return clear of the lenders' legs: no arrowhead lies
// along a leg into the dot it points at, and no shaft leaves a dot along
// one.

// The issue's program: pair gives a and b back on the `else` path.
fn one_line() {
    let a = String::from("a");
    let b = String::from("b");
    let pair = Some((&a, &b));
    let Some((x, y)) = pair else { panic!() };
    println!("{} {} {} {}", x, y, a, b);
}

// The `else` block on lines of its own.
fn multi_line() {
    let a = String::from("a");
    let b = String::from("b");
    let pair = Some((&a, &b));
    let Some((x, y)) = pair else {
        panic!()
    };
    println!("{} {} {} {}", x, y, a, b);
}

// Mutable loans.
fn mutable() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let pair = Some((&mut a, &mut b));
    let Some((x, y)) = pair else { panic!() };
    x.push('!');
    y.push('!');
    println!("{} {}", a, b);
}

// An `if let` and a `match` on the holder.
fn if_let_and_match(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let pair = if c { Some((&a, &b)) } else { None };
    if let Some((x, y)) = pair {
        println!("{} {}", x, y);
    }
    match pair {
        Some((x, _)) => println!("{}", x),
        None => {}
    }
    println!("{} {}", a, b);
}

fn main() {
    one_line();
    multi_line();
    mutable();
    if_let_and_match(true);
}
