#![allow(dead_code, unused_variables, unused_assignments, irrefutable_let_patterns, dropping_copy_types)]

fn shared(c: bool, o: Option<()>, s: String) {
    if c {
        if let Some(()) = o {
            drop(s);
        } else {
            drop(s);
        }
    }
}

fn imaginary(c: bool, s: String) {
    if let true | false = c {
        drop(s);
    }
}

fn pattern(opt: Option<String>) {
    let Some(mut s) = opt else { return };
    s = String::from("replacement");
    drop(s);
}

fn staged(c: bool, a: u32, b: u32, other: u32) {
    let (x, y);
    (x, y) = (other, if c { a } else { b });
    drop((x, y));
}

fn aggregate(c: bool, a: u32, b: u32, z: u32, w: u32) {
    let tuple = ((if c { a } else { b }, z), w);
    drop(tuple);
}

fn partial(mut pair: (Option<String>, String), c: bool) {
    if c {
        if let Some(s) = pair.0 { drop(s); }
    }
    pair.1.push('!');
}

fn in_test(mut s: String, c: bool) {
    if { s = String::from("replacement"); c } {
        s.push('t');
    } else {
        s.push('f');
    }
    drop(s);
}

fn leaver(mut s: String, c: bool, d: bool) {
    if c {
        drop(s);
        s = String::from("replacement");
    } else if d {
        drop(s);
        return;
    }
    drop(s);
}

fn inherited(mut s: String, c: bool) {
    drop(s);
    if c {
        s = String::from("replacement");
        drop(s);
    } else {
        println!("empty");
    }
}

fn scrutinee(mut s: String, c: bool, d: bool) {
    match c && { drop(s); d } {
        true => { s = String::from("replacement"); }
        false => (),
    }
}

fn self_assignment(mut s: String, c: bool) {
    if c {
        s = s;
    } else {
        drop(s);
    }
}

fn rc(mut p: std::rc::Rc<String>, c: u8) {
    match c {
        0 => { drop(p); p = std::rc::Rc::new(String::from("replacement")); }
        1 => drop(p),
        _ => (),
    }
}

fn shared_residual(c: bool, a: Result<(), ()>, b: Result<(), ()>, shared: String) -> Result<(), ()> {
    if c { a?; } else { b?; }
    drop(shared);
    Ok(())
}

fn same_row(c: bool, a: Result<(), ()>, b: Result<(), ()>, local: String) -> Result<(), ()> {
    a?; if c { drop(local); } b?;
    Ok(())
}

fn empty(a: Result<(), ()>, local: String) -> Result<(), ()> {
    drop(local);
    a?;
    Ok(())
}

fn split_write(mut s: String, c: bool, d: bool) {
    if c && { s = String::from("during the test"); d } {
        s.push('t');
    } else {
        s = String::from("in the arm");
    }
    drop(s);
}

fn main() {}
