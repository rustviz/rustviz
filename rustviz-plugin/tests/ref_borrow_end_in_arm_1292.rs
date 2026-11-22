// #1292: a reference declared before a conditional whose borrow ends in
// one arm (its last use there) draws the rest of that arm dashed ("c's
// borrow has ended on this path") down to the join. The dashes start at
// the dot where the borrow ends; they used to start a row lower, in
// mid-air. Each function is one context: `if`/`else`, inside a `for`
// loop, a `match` arm, an `else if` chain, a closure body, a `&mut`
// reference, an `if let`, and a nested `if`.

fn if_else(flag: bool) {
    let y = 9;
    let x = 8;
    let c = &x;
    if flag {
        println!("{}", c);
        println!("done");
    } else {
        println!("{}", y);
    }
}

fn in_loop(v: &Vec<i32>) {
    let x = 1;
    let mut r = &x;
    for i in v {
        if *i > 0 {
            println!("{}", r);
            println!("pos");
        } else {
            r = i;
        }
    }
}

fn in_match(n: i32) {
    let s = String::from("s");
    let r = &s;
    match n {
        0 => {
            println!("{}", r);
            println!("zero");
        }
        _ => {
            println!("other");
        }
    }
}

fn else_if(n: i32) {
    let s = String::from("s");
    let r = &s;
    if n == 0 {
        println!("{}", r);
        println!("zero");
    } else if n == 1 {
        println!("one");
    } else {
        println!("many");
    }
}

fn in_closure() {
    let f = |c: bool| {
        let s = String::from("s");
        let r = &s;
        if c {
            println!("{}", r);
            println!("c");
        } else {
            println!("not c");
        }
    };
    f(true);
}

fn mut_ref(c: bool) {
    let mut s = String::from("s");
    let r = &mut s;
    if c {
        r.push('x');
        println!("c");
    } else {
        println!("not c");
    }
    s.push('y');
}

fn if_let(o: Option<i32>) {
    let s = String::from("s");
    let r = &s;
    if let Some(n) = o {
        println!("{} {}", r, n);
        println!("some");
    } else {
        println!("none");
    }
}

fn nested_if(c: bool, d: bool) {
    let s = String::from("hi");
    let r = &s;
    if c {
        if d {
            println!("{}", r);
        } else {
            println!("d");
        }
        println!("after");
    } else {
        println!("c");
    }
}

fn main() {
    if_else(true);
    in_loop(&vec![1]);
    in_match(0);
    else_if(0);
    in_closure();
    mut_ref(true);
    if_let(Some(1));
    nested_if(true, true);
}
