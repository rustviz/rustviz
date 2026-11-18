// An exit after a move and a nested conditional that gives the variable
// a new value on every path (#1135): it owns the new value at the exit,
// which drops it.

fn ret(c1: bool, c2: bool) {
    let mut a = String::from("init");
    if c1 {
        drop(a);
        if c2 {
            a = String::from("x");
        } else {
            a = String::from("y");
        }
        return;
    } else {
        println!("{}", a);
    }
}

fn panics(c1: bool, c2: bool) {
    let mut b = String::from("init");
    if c1 {
        drop(b);
        match c2 {
            true => b = String::from("x"),
            false => b = String::from("y"),
        }
        panic!("stop");
    }
    println!("{}", b);
}

fn deeper(c1: bool, c2: bool, c3: bool) {
    let mut d = String::from("init");
    if c1 {
        drop(d);
        if c2 {
            if c3 {
                d = String::from("x");
            } else {
                d = String::from("y");
            }
        } else {
            d = String::from("z");
        }
        return;
    }
    println!("{}", d);
}

fn in_arm(c1: bool, o: Option<String>) {
    let mut e = String::from("init");
    match c1 {
        true => {
            drop(e);
            if let Some(x) = o {
                e = x;
            } else {
                e = String::new();
            }
            return;
        }
        false => println!("{}", e),
    }
}

fn next(v: Vec<bool>, c2: bool) {
    for c in v {
        let mut f = String::from("init");
        if c {
            drop(f);
            if c2 {
                f = String::from("x");
            } else {
                f = String::from("y");
            }
            continue;
        }
        println!("{}", f);
    }
}

fn main() {
    ret(true, true);
    in_arm(true, None);
    deeper(true, true, true);
    next(vec![true], true);
    panics(false, true);
}
