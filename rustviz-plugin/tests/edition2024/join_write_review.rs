fn consume(s: String) -> bool { !s.is_empty() }

pub fn chain(input: Option<String>, stop: bool) {
    let mut s = String::from("old");
    if let Some(()) = Some(()) && consume(s) && let Some(x) = input && { s = x; !s.is_empty() } {
        println!("consumed");
    } else {
        println!("end");
    }
    let _ = stop && { s = String::new(); true };
}

pub fn nested(c: bool, restore: bool, outer: bool, stop: bool) {
    let mut s = String::from("old");
    if c { drop(s); }
    if restore { s = String::from("replacement"); }
    if outer {
        let _ = stop && { s = String::new(); true };
        println!("if arm");
    } else {
        match restore {
            true => {
                let _ = stop && { s = String::new(); true };
                println!("match arm");
            }
            false => (),
        }
    }
}

pub fn unwind(c: bool, fail: bool) {
    let mut s = String::from("old");
    if c { drop(s); }
    if fail { panic!(); }
    s = String::new();
    drop(s);
}

pub fn first_assignment(c: bool) {
    let s: String;
    'blk: {
        if c { break 'blk; }
        s = String::from("new");
    }
    println!("end");
}
