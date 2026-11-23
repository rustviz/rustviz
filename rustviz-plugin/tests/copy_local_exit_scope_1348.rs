// A Copy value or a reference declared in a block that leaves early
// (`return`, `break`, `continue`, a panic, `process::exit`) goes out of
// scope there, like a value that is dropped there, not at the block's
// closing brace, which that path never reaches (#1348).
fn let_else(opt: Option<i32>) {
    let Some(x) = opt else {
        let n = 5;
        let s = String::from("s");
        return;
    };
    println!("{}", x);
}
fn if_return(c: bool) {
    if c {
        let n = 5;
        let s = String::from("s");
        return;
    }
    println!("{}", c);
}
fn arm_return(o: Option<i32>) -> i32 {
    match o {
        Some(v) => v,
        None => {
            let k = 1;
            let r = &k;
            println!("{}", r);
            return 0;
        }
    }
}
fn loop_exits(v: Vec<i32>) {
    for x in v {
        if x > 2 {
            let n = x;
            break;
        }
        if x > 1 {
            let m = x;
            continue;
        }
    }
}
fn nested(c: bool, d: bool) {
    if c {
        let n = 1;
        if d { return; } else { return; }
    } else {
        let p = 2;
        panic!("p");
    }
}
fn exits(c: bool) {
    if c {
        let q = 3;
        std::process::exit(1);
    }
    println!("{}", c);
}
fn main() {
    let_else(Some(1));
    if_return(false);
    arm_return(None);
    loop_exits(vec![1, 2, 3]);
    exits(false);
    nested(false, false);
}
