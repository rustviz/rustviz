// #1436: a `let x;` local that only some branches above assigned (or a
// temporary a `let` extends through a branch, #1397) has a value on some
// paths only, and Rust drops it with a drop flag. An exit after that
// conditional (a `return`, a `?`, a `break` or `continue` out of the loop
// it is declared in, a panic) drops it if it has a value. That drop wasn't
// stated anywhere: the local's entry in the list of what an exit drops
// went with the branch that assigned it. A local every branch assigned
// wasn't dropped by a later exit either.
fn bar() -> Result<String, ()> {
    Ok(String::new())
}

fn at_return(c: bool, d: bool) {
    let s;
    if c {
        s = String::from("x");
        println!("{}", s);
    }
    if d {
        return;
    }
    println!("end");
}

fn at_try(c: bool) -> Result<(), ()> {
    let s;
    if c {
        s = String::from("x");
        println!("{}", s);
    }
    let _t = bar()?;
    Ok(())
}

fn in_loop(v: Vec<i32>) {
    for x in v {
        let s;
        if x > 1 {
            s = String::from("x");
            println!("{}", s);
        }
        if x > 2 {
            break;
        }
        if x > 3 {
            continue;
        }
        println!("next");
    }
}

fn at_panic(c: bool, d: bool) {
    let s;
    if c {
        s = String::from("x");
        println!("{}", s);
    }
    if d {
        panic!("no");
    }
}

fn after_match(c: bool, n: i32) {
    let s;
    match n {
        0 => {
            s = String::from("a");
            println!("{}", s);
        }
        _ => {}
    }
    if c {
        return;
    }
}

struct P {
    name: String,
}

fn whole_struct(c: bool, d: bool) {
    let p;
    if c {
        p = P { name: String::from("x") };
        println!("{}", p.name);
    }
    if d {
        return;
    }
}

// Every branch assigned it: the exit drops it.
fn every_branch(c: bool, d: bool) {
    let s;
    if c {
        s = String::from("x");
    } else {
        s = String::from("y");
    }
    if d {
        return;
    }
    println!("{}", s);
}

fn temp_try(c: bool) -> Result<(), ()> {
    let s = String::from("s");
    let r = if c { &String::from("x") } else { &s };
    println!("{}", r);
    let _t = bar()?;
    Ok(())
}

fn temp_break(v: Vec<i32>) {
    let s = String::from("s");
    for x in v {
        let r = if x > 0 { &String::from("x") } else { &s };
        println!("{}", r);
        if x > 2 {
            break;
        }
    }
}

fn main() {
    at_return(true, false);
    let _ = at_try(true);
    in_loop(vec![1]);
    at_panic(true, false);
    after_match(true, 0);
    whole_struct(true, false);
    every_branch(true, false);
    let _ = temp_try(true);
    temp_break(vec![1]);
}
