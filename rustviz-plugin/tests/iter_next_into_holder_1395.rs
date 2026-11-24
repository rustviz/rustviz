// #1395: `refs.iter_mut().next().unwrap()` with `refs: Vec<&String>`
// points into refs, as `refs.last_mut().unwrap()` (#1382) and `&mut
// refs[0]` do: `next` returns `Option<&'a mut T>` for the `IterMut<'a,
// T>` that `iter_mut` returns, whose `'a` borrows refs; the `&String`
// it points to borrows a1, but el reaches a1 only through refs. el was
// drawn borrowing a1 too, mutably ("so it may point into either"),
// though a1 is only ever lent immutably, to refs.
fn f1() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let el = refs.iter_mut().next().unwrap();
    *el = &t1;
    println!("{:?}", refs);
}

// `nth`, `last` (which takes the iterator by value), `expect`.
fn f2() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let el = refs.iter_mut().nth(0).expect("one");
    *el = &t1;
    let el2 = refs.iter_mut().last().unwrap();
    *el2 = &a1;
    println!("{:?}", refs);
}

// `iter`, and `into_iter` on `&refs` / `&mut refs`.
fn f3() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let r1 = refs.iter().next().unwrap();
    println!("{}", r1);
    let r2 = (&refs).into_iter().nth(0).unwrap();
    println!("{}", r2);
    let r3 = (&mut refs).into_iter().next().unwrap();
    *r3 = &t1;
    println!("{:?}", refs);
}

// The `&String` copied out of the element borrows a1, not refs: refs
// can be pushed to while x is in use.
fn f4() {
    let a1 = String::from("a1");
    let a2 = String::from("a2");
    let mut refs = vec![&a1];
    let x = refs.iter().next().copied().unwrap();
    refs.push(&a2);
    println!("{} {:?}", x, refs);
}

// Kept in an `Option` (also `last_mut`'s, #1382), the same binding
// name in both (#1400).
fn f5() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    let o = refs.iter_mut().next();
    if let Some(e) = o {
        *e = &t1;
    }
    let p = refs.last_mut();
    if let Some(e) = p {
        *e = &a1;
    }
    println!("{:?}", refs);
}

// An array.
fn f6() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut arr = [&a1];
    let el = arr.iter_mut().next().unwrap();
    *el = &t1;
    println!("{:?}", arr);
}

fn main() {
    f1();
    f2();
    f3();
    f4();
    f5();
    f6();
}
