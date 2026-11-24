// #1399: an iterator over a `Vec<&String>` kept in a local, or passed
// through adapters, points into refs, as `refs.iter_mut().next()
// .unwrap()` does (#1395): `it` is an `IterMut<'a, &String>` whose `'a`
// borrows refs mutably; the `&String`s in refs borrow a1, but `it` and
// the `el` taken out of it reach a1 only through refs. They were drawn
// borrowing a1 too, mutably ("so it may borrow from either"), though
// a1 is only ever lent immutably, to refs.
fn local() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let mut it = refs.iter_mut();
    let el = it.next().unwrap();
    *el = &t1;
    println!("{:?}", refs);
}

// Adapters, with no local and in one.
fn adapters() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let el = refs.iter_mut().rev().next().unwrap();
    *el = &t1;
    let e2 = refs.iter_mut().skip(0).next().unwrap();
    *e2 = &a1;
    let mut it = refs.iter_mut().skip(0).take(2).rev();
    let e3 = it.next().unwrap();
    *e3 = &t1;
    println!("{:?}", refs);
}

// `if let`, `while let`, `match`, `let ... else` on `it.next()`.
fn patterns() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let mut it = refs.iter_mut();
    if let Some(el) = it.next() {
        *el = &t1;
    }
    let mut it2 = refs.iter_mut();
    while let Some(e2) = it2.next() {
        *e2 = &a1;
    }
    let mut it3 = refs.iter_mut();
    match it3.next() {
        Some(e3) => *e3 = &t1,
        None => {}
    }
    let mut it4 = refs.iter_mut();
    let Some(e4) = it4.next() else { return };
    *e4 = &a1;
    println!("{:?}", refs);
}

// Moved to another variable, kept in an `Option`, `nth`, `by_ref`,
// `enumerate`, `peekable`, `into_iter` on `&mut refs`.
fn more() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let it = refs.iter_mut();
    let mut it2 = it;
    let el = it2.next().unwrap();
    *el = &t1;
    let mut it3 = refs.iter_mut();
    let o = it3.next();
    if let Some(e2) = o {
        *e2 = &a1;
    }
    let mut it4 = refs.iter_mut();
    let e3 = it4.nth(0).unwrap();
    *e3 = &t1;
    let mut it5 = refs.iter_mut();
    let e4 = it5.by_ref().next().unwrap();
    *e4 = &a1;
    let mut it6 = refs.iter_mut().enumerate();
    let (i, e5) = it6.next().unwrap();
    *e5 = &t1;
    let mut it7 = refs.iter_mut().peekable();
    let e6 = it7.next().unwrap();
    *e6 = &a1;
    let mut it8 = (&mut refs).into_iter();
    let e7 = it8.next().unwrap();
    *e7 = &t1;
    println!("{} {:?}", i, refs);
}

fn set<'a>(el: &mut &'a String, t: &'a String) {
    *el = t;
}

// A call argument, a write through the reference with no variable, an
// array, a `Vec<&mut String>`.
fn other() {
    let mut a1 = String::from("a1");
    let t1 = String::from("t1");
    let a2 = String::from("a2");
    let mut refs = vec![&a2];
    let mut it = refs.iter_mut();
    set(it.next().unwrap(), &t1);
    let mut it2 = refs.iter_mut();
    *it2.next().unwrap() = &a2;
    let mut arr = [&a2];
    let mut it3 = arr.iter_mut();
    let el = it3.next().unwrap();
    *el = &t1;
    let mut muts = vec![&mut a1];
    let mut it4 = muts.iter_mut();
    let e2 = it4.next().unwrap();
    e2.push('x');
    println!("{:?} {:?} {:?}", refs, arr, muts);
}

fn main() {
    local();
    adapters();
    patterns();
    more();
    other();
}
