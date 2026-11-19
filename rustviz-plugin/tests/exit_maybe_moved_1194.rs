// #1194: a variable moved (or given a value) on some paths only is still
// dropped by an exit or a `?` on the paths where it holds one: Rust
// decides with a runtime drop flag. Each exit states that drop, as the
// variable's scope end does.
fn bar() -> Result<String, ()> {
    Ok(String::new())
}

fn at_return(c1: bool, c2: bool) {
    let s = String::from("init");
    if c1 {
        if c2 {
            drop(s);
        }
        return;
    }
    println!("{}", s);
}

fn at_try(c: bool) -> Result<(), ()> {
    let mut t = String::from("a");
    if c {
        drop(t);
    }
    t = bar()?;
    println!("{}", t);
    Ok(())
}

fn try_plain(c: bool) -> Result<(), ()> {
    let u = String::from("a");
    if c {
        drop(u);
    }
    let _w = bar()?;
    Ok(())
}

// May hold nothing: given a value on some paths only.
fn maybe_init(c1: bool, c2: bool) {
    let mut m = String::from("a");
    drop(m);
    if c1 {
        if c2 {
            m = String::from("b");
        }
        return;
    }
    m = String::from("c");
    println!("{}", m);
}

fn at_panic(c1: bool, c2: bool) {
    let p = String::from("init");
    if c1 {
        if c2 {
            drop(p);
        }
        panic!("x");
    }
    println!("{}", p);
}

fn at_break(v: Vec<i32>) {
    for x in v {
        let k = String::from("a");
        if x > 1 {
            if x > 2 {
                drop(k);
            }
            break;
        }
        println!("{}", k);
    }
}

// The conditional that moved it is above the one the exit is in.
fn in_arm(c: bool, n: i32) -> Result<(), ()> {
    let a = String::from("a");
    if c {
        drop(a);
    }
    match n {
        0 => return Ok(()),
        1 => {
            bar()?;
        }
        _ => {}
    }
    Ok(())
}

fn let_else(c: bool, o: Option<i32>) {
    let b = Box::new(String::from("b"));
    if c {
        drop(b);
    }
    let Some(_y) = o else {
        return;
    };
}

fn main() {
    at_return(true, false);
    let _ = at_try(false);
    let _ = try_plain(false);
    maybe_init(false, false);
    at_panic(false, false);
    at_break(vec![1]);
    let _ = in_arm(false, 2);
    let_else(false, Some(1));
}
