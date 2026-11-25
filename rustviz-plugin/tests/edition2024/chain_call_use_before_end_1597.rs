// #1597: everything on one line. x borrows o, `len` reads through x, x's
// loan ends (its last use), and only then does `as_mut` borrow o mutably
// for m. x's dot listed the `len` read before x's borrow, and o's listed
// the mutable borrows before x's loan ended.
fn chain_mut_body() {
    let mut o = Some(String::from("rust"));
    if let Some(x) = &o && x.len() > 1 { let m = o.as_mut().unwrap(); }
}

// The same order outside a conditional: r's loan ends after `r.len()`,
// before m's mutable borrow of s.
fn plain_lets() {
    let mut s = String::from("a");
    let r = &s; let n = r.len(); let m = &mut s;
    m.push('b');
}

fn chain_push() {
    let mut v = vec![String::from("a")];
    if let Some(x) = v.first() && x.len() > 0 { v.push(String::new()); }
}

fn main() {}
