// A `&mut` moved into a call in one arm (`take_gen(m)`, `drop(m)`) ends
// its borrow there, and the other arms end it too. With no join dot for
// m (one path moved it), the other arms' dashed "m's borrow has ended on
// this path" legs ran on to the join row and stopped in mid-air (#1416).
fn take_gen<T>(_t: T) {}

fn if_without_else() {
    let mut s = String::from("a");
    let m = &mut s;
    let c = true;
    if c {
        take_gen(m);
    }
    println!("{}", s);
}

fn if_else() {
    let mut s = String::from("a");
    let m = &mut s;
    let c = true;
    if c {
        take_gen(m);
    } else {
        m.push('b');
    }
    println!("{}", s);
}

fn if_let() {
    let mut s = String::from("a");
    let m = &mut s;
    let o = Some(1);
    if let Some(_) = o {
        take_gen(m);
    }
    println!("{}", s);
}

fn else_if() {
    let mut s = String::from("a");
    let m = &mut s;
    let n = 3;
    if n > 2 {
        take_gen(m);
    } else if n > 1 {
        m.push('x');
    } else {
    }
    println!("{}", s);
}

fn nested() {
    let mut s = String::from("a");
    let m = &mut s;
    let c = true;
    if c {
        if c {
            take_gen(m);
        }
    }
    println!("{}", s);
}

fn guarded_match(m: &mut String) -> Option<usize> {
    let o: Option<String> = None;
    match o {
        Some(x) if x.len() > 1 => {}
        Some(_) => drop(m),
        None => {}
    }
    Some(0)
}

fn main() {
    if_without_else();
    if_else();
    if_let();
    else_if();
    nested();
    guarded_match(&mut String::new());
}
