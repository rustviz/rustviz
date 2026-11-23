// #1294: an `else if` is a conditional nested in the outer one's `else`
// arm that ends on the same `}` row. The outer arm's join leg started in
// mid-air beside the inner join (drawn from the row above it); now the
// joins of a chain share the row above the `}` (the inner one higher up)
// and each `else` arm's leg starts at the inner join's dot. Chains of
// three and four arms, references re-pointed in the arms, moves, a `let`
// bound from the chain, `if let … else if let`, in a loop, with a `return`.
fn take(s: String) {
    println!("{}", s);
}

fn issue() {
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
    // (Used after the chain, so c's borrows don't end in the arms: that
    // draws #1292's gap.)
    println!("{}", c);
}

fn four(n: i32) {
    let s = String::from("a");
    let t = String::from("b");
    let mut r = &t;
    if n == 0 {
        println!("zero");
    } else if n == 1 {
        r = &s;
    } else if n == 2 {
        println!("{}", s);
    } else {
        take(s);
    }
    println!("{}", r);
}
fn moves(n: i32) {
    let s = String::from("a");
    if n == 0 {
        take(s);
    } else if n == 1 {
        println!("{}", s);
    } else {
        println!("x");
    }
}
fn let_rhs(n: i32) {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let x = if n == 0 {
        a
    } else if n == 1 {
        b
    } else {
        c
    };
    println!("{}", x);
}
fn if_let_chain(o: Option<i32>, p: Option<i32>) {
    let s = String::from("a");
    if let Some(_) = o {
        println!("{}", s);
    } else if let Some(_) = p {
        take(s);
    } else {
        println!("{}", s);
    }
}
fn in_loop(v: Vec<i32>) {
    let mut s = String::from("a");
    for n in v {
        if n == 0 {
            s.push('a');
        } else if n == 1 {
            println!("{}", s);
        } else {
            s.push('c');
        }
    }
}
fn returns(n: i32) -> String {
    let s = String::from("a");
    if n == 0 {
        return s;
    } else if n == 1 {
        println!("{}", s);
    } else {
        return String::new();
    }
    s
}
fn main() { issue(); four(1); moves(0); let_rhs(0); if_let_chain(None, None); in_loop(vec![1]); returns(1); }
