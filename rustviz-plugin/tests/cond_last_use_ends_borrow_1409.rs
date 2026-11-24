// #1409: a borrow whose last use is an `if` / `match` condition ends
// there, before either arm runs: the lender is drawn as its arm legs from
// the fork, the owner again in each, not as a column running on beside
// them, still lent (and `State::Invalid` in an arm using it after a
// `&mut`).
fn shared_if_else() {
    let mut s = String::from("a");
    let r = &s;
    if r.len() > 2 {
        s.push('x');
    } else {
        println!("{}", s);
    }
}

fn mut_if_else() {
    let mut s = String::from("a");
    let m = &mut s;
    if m.len() > 2 {
    } else {
        println!("{}", s);
    }
}

fn match_scrutinee() {
    let mut s = String::from("a");
    let r = &s;
    match r.len() {
        0 => s.push('x'),
        _ => println!("{}", s),
    }
}

fn if_let() {
    let mut s = String::from("ab");
    let r = &s;
    if let Some(c) = r.chars().next() {
        s.push(c);
    } else {
        println!("{}", s);
    }
}

fn holder() {
    let mut v = vec![1, 2];
    let it = v.iter();
    if it.len() > 1 {
        v.push(3);
    } else {
        println!("{:?}", v);
    }
}

fn option_as_mut() {
    let mut o = Some(String::from("a"));
    let m = o.as_mut().unwrap();
    if m.len() > 2 {
        o = None;
    } else {
        println!("{:?}", o);
    }
    println!("{:?}", o);
}

// The arm moves s: the borrow ended before that, not at the join.
fn value_if() {
    let s = String::from("a");
    let r = &s;
    let t = if r.len() > 2 { s } else { String::new() };
    println!("{}", t);
}

fn one_line() {
    let mut s = String::from("a");
    let r = &s;
    if r.len() > 2 { s.push('x'); } else { s.push('y'); }
    println!("{}", s);
}

fn main() {
    shared_if_else();
    mut_if_else();
    match_scrutinee();
    if_let();
    holder();
    option_as_mut();
    value_if();
    one_line();
}
