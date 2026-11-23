// #1326: a reference whose borrow ends in every arm of an `else if` (or
// of an `if` / `match` that ends the outer `else`): that conditional's
// join says so, and the outer `else` arm's leg runs on from it to the
// outer join, dashed ("r's borrow has ended on this path"), as the
// lender's columns do. main drew the inner join with no leg on.
fn bump(r: &mut i32) -> bool { *r += 1; *r > 2 }
fn look(r: &i32) -> bool { *r > 2 }

// A let-chain `else if` (the issue's case).
fn chain_else_if(go: bool, o: Option<i32>) {
    let mut e = 0;
    let r6 = &mut e;
    if go {
        println!("go");
    } else if let Some(x) = o && bump(r6) {
        println!("{}", x);
    } else {
        println!("no");
    }
    println!("{}", e);
}

// A plain `else if`, the borrow used in both inner arms.
fn plain_else_if(go: bool, c: bool) {
    let mut e = 0;
    let r = &mut e;
    if go {
        println!("go");
    } else if c {
        bump(r);
    } else {
        bump(r);
    }
    println!("{}", e);
}

// A shared reference.
fn shared(go: bool, o: Option<i32>) {
    let e = 0;
    let r = &e;
    if go {
        println!("go");
    } else if let Some(x) = o && look(r) {
        println!("{}", x);
    } else {
        println!("no");
    }
    println!("{}", e);
}

// An `if` / `else` inside the outer `else`, with code after it.
fn nested_not_last(go: bool, c: bool) {
    let mut e = 0;
    let r = &mut e;
    if go {
        println!("go");
    } else {
        if c {
            bump(r);
        } else {
            bump(r);
        }
        println!("after");
    }
    println!("{}", e);
}

// A `match` inside the outer `else`.
fn match_in_else(go: bool, n: u8) {
    let mut e = 0;
    let r = &mut e;
    if go {
        println!("go");
    } else {
        match n {
            0 => { bump(r); }
            _ => { bump(r); }
        }
    }
    println!("{}", e);
}

// The borrow still live after the conditional: no inner join for it
// (unchanged).
fn still_live(go: bool, c: bool) {
    let mut e = 0;
    let r = &mut e;
    if go {
        println!("go");
    } else if c {
        bump(r);
    } else {
        println!("no");
    }
    bump(r);
}

fn main() {
    chain_else_if(false, Some(3));
    plain_else_if(false, true);
    shared(false, None);
    nested_not_last(false, true);
    match_in_else(false, 0);
    still_live(false, true);
}
