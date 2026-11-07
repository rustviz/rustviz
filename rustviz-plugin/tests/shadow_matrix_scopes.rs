// Shadowing matrix, part 2: shadowing in inner scopes and fns. An
// inner `let` ends with its block; the outer variable is visible again
// after it, and its uses go to its own column.

// An inner block shadows `a`, then the outer `a` is moved after it.
fn block_then_outer() {
    let a = String::from("outer");
    {
        let a = a.len();
        println!("{}", a);
    }
    let t = a;
    println!("{}", t);
}

// A `let` in an `if` arm shadows `b`; the outer `b` is used after.
fn if_arm(c: bool) {
    let b = String::from("outer");
    if c {
        let b = String::from("inner");
        println!("{}", b);
    }
    println!("{}", b);
}

// A `let` in a loop body shadows `l`; the outer `l` is used after.
fn loop_body() {
    let l = String::from("outer");
    for _ in 0..2 {
        let l = String::from("inner");
        println!("{}", l);
    }
    println!("{}", l);
}

// A parameter shadowed in the body.
fn param(s: String) -> String {
    let s = s + "x";
    s
}

// A `&str` parameter shadowed by an owned String.
fn ref_param(v: &str) -> usize {
    let v = v.to_string();
    v.len()
}

// `return r;` returns the second r; the first is dropped at each
// `return`. (The merge and the closing brace still draw drops that no
// path reaches, #515.)
fn early_return(c: bool) -> String {
    let r = String::from("first");
    let r = String::from("second");
    if c {
        return r;
    } else {
        return r + "!";
    }
}

// Each fn numbers its own variables: this `s` is unrelated to
// `param`'s.
fn main() {
    let s = String::from("m");
    let s = param(s);
    println!("{} {}", s, ref_param(&s));
    block_then_outer();
    if_arm(true);
    loop_body();
    early_return(true);
}
