// #1447: a value holding a borrow (an iterator, an `Option<&mut T>`)
// whose scrutinee (`it.next()`, or the holder itself) a pattern takes a
// reference out of. On the path through that arm the binding gives v
// back at its last use (rustc ends the loan there); on the paths
// through the other arms the loan ended at the scrutinee. The holder's
// own end gave v back a second time: beside the arms (on v's column
// between them, `match it.next()`), or at the join.
fn iter_match() {
    let mut v = vec![String::new()];
    let mut it = v.iter_mut();
    match it.next() {
        Some(e) => e.push('x'),
        None => {}
    }
    println!("{:?}", v);
}

// Shared, with work in the other arm.
fn iter_match_shared() {
    let v = vec![String::new()];
    let mut it = v.iter();
    match it.next() {
        Some(e) => println!("{}", e),
        None => println!("none"),
    }
    println!("{:?}", v);
}

// `if let` with an `else`.
fn iter_if_let_else() {
    let mut v = vec![String::new()];
    let mut it = v.iter_mut();
    if let Some(e) = it.next() {
        e.push('x');
    } else {
        println!("empty");
    }
    println!("{:?}", v);
}

// What next returned, kept in o first.
fn option_of_next() {
    let mut v = vec![String::new()];
    let mut it = v.iter_mut();
    let o = it.next();
    if let Some(e) = o {
        e.push('y');
    }
    println!("{:?}", v);
}

// The holder as the scrutinee, its arms on one line.
fn one_line_match() {
    let mut v = vec![String::new()];
    let o = v.last_mut();
    match o { Some(e) => e.push('z'), None => {} }
    println!("{:?}", v);
}

// The holder used again after the conditional: it keeps v borrowed, and
// e's end gives nothing back (#575).
fn used_after() {
    let mut v = vec![String::new()];
    let mut it2 = v.iter_mut();
    if let Some(e) = it2.next() {
        e.push('x');
    }
    it2.next();
    println!("{:?}", v);
}

fn main() {
    iter_match();
    iter_match_shared();
    iter_if_let_else();
    option_of_next();
    one_line_match();
    used_after();
}
