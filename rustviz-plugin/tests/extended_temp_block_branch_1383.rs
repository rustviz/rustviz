// #1383: Rust extends a temporary borrowed in a block's last expression,
// an `if` / `else` branch or a `match` arm of a `let` initializer to the
// end of the enclosing block, as it does for `let r = &String::from("x");`.
// A block's gets a `(temporary)` column; one made in a branch is made on
// that path only (a drop flag decides its drop), which isn't drawn: a
// note says where it is dropped.
fn e1(c: bool) {
    let s = String::from("s");
    let r = if c { &String::from("x") } else { &s };
    println!("{}", r);
}

fn e2(c: bool) {
    let t = if c { Some(&String::from("x")) } else { None };
    println!("{:?}", t);
}

fn e3() {
    let r = { &String::from("x") };
    println!("{}", r);
}

fn e4() {
    let t = { Some(&String::from("x")) };
    println!("{:?}", t);
}

fn e5() {
    let r = {
        let n = 1;
        println!("{}", n);
        &String::from("x")
    };
    println!("{}", r);
}

fn m1(o: Option<i32>) {
    let s = String::from("s");
    let r = match o {
        Some(_) => &String::from("x"),
        None => &s,
    };
    println!("{}", r);
}

fn m2(c: bool) {
    let s = String::from("s");
    let r = if c {
        &s
    } else {
        &String::from("y")
    };
    println!("{}", r);
}

fn main() {
    e1(true);
    e2(true);
    e3();
    e4();
    e5();
    m1(Some(1));
    m2(false);
}
