// #1548: `if let Some(n) = Some(id(r))`: the pattern binds n to the
// reference inside the value `Some(..)` built, and with it n borrows s
// (through r). One dot: main drew n's borrow of s a row above the dot
// that gave n its value, so n borrowed s before it had a value.
fn id(s: &String) -> &String {
    s
}

fn downgrade(s: &mut String) -> &String {
    s
}

fn main() {
    let s = String::from("x");
    let r = &s;
    if let Some(n) = Some(id(r)) {
        println!("{}", n);
    }
    if let Some(m) = Some(r) {
        println!("{}", m);
    }
    while let Some(w) = Some(id(r)) {
        println!("{}", w);
        break;
    }
    mutable();
}

// The loan r holds is a mutable one (`downgrade(&mut t)`): n2 borrows t
// mutably, on the row it is bound on.
fn mutable() {
    let mut t = String::from("x");
    let r2 = downgrade(&mut t);
    if let Some(n2) = Some(id(r2)) {
        println!("{}", n2);
    }
}

// #1563: a method's returned reference put into the scrutinee's value
// is named "the reference trim returns".
fn trimmed() {
    let u = String::from(" x ");
    let x = &u;
    if let Some(p) = Some(x.trim()) {
        println!("{}", p);
    }
}
