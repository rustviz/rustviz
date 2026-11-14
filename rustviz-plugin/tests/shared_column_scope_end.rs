// #931: variables of one name that are never in scope together share a
// column. When one of them was declared inside a branch or a loop body
// and the other wasn't, the second one's scope end wasn't drawn: an
// owner was never said to be dropped. Each one now goes out of scope
// where it would with a name of its own.

// The later `s` is in a block inside a one-armed `if`.
fn main() {
    let n = 3;
    {
        let s = String::from("a");
        println!("first {}", s);
    }
    if n > 0 {
        {
            let s = String::from("b");
            println!("second {}", s);
        }
        println!("x");
    }
}

// The earlier `r` is in a `match` arm, the later one in a block at the
// function's level.
fn after_arm(n: i32) {
    let a = String::from("a");
    match n {
        0 => {
            let r = &a;
            println!("first {}", r);
        }
        _ => {}
    }
    {
        let r = &a;
        println!("second {}", r);
    }
}

// The later `u` is in a `while` body.
fn in_loop(mut n: i32) {
    {
        let u = String::from("a");
        println!("first {}", u);
    }
    while n > 0 {
        let u = String::from("b");
        println!("second {}", u);
        n -= 1;
    }
}

// The earlier `q` is in a `while` body, the later one at the function's
// level.
fn loop_first(mut n: i32) {
    while n > 0 {
        let q = String::from("a");
        println!("first {}", q);
        n -= 1;
    }
    let q = String::from("b");
    println!("second {}", q);
}

// A struct, whose field columns go out of scope with it.
struct P {
    name: String,
    n: i32,
}

fn fields(n: i32) {
    {
        let p = P { name: String::from("a"), n: 1 };
        println!("first {} {}", p.name, p.n);
    }
    if n > 0 {
        {
            let p = P { name: String::from("b"), n: 2 };
            println!("second {} {}", p.name, p.n);
        }
        println!("x");
    }
}
