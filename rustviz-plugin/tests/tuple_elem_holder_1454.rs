// #1454: a tuple element whose value holds a borrow (`Some(&s1)`) holds
// it, as a struct literal's field does: s1 stays borrowed while t is in
// use (`s1.push_str` before the `println!` is E0502).
#[derive(Debug)]
struct W<'a> {
    o: Option<&'a String>,
}

fn main() {
    let mut s1 = String::from("hi");
    let s2 = String::from("yo");
    let n = s1.len();
    let t = (Some(&s1), 1);
    // From a conditional: u.0 may hold s1's or s2's reference.
    let u = (if n > 2 { Some(&s1) } else { Some(&s2) }, 1);
    println!("{:?} {:?}", t, u);

    // Assigned.
    let mut a = (None, 0);
    println!("{:?}", a);
    a = (Some(&s2), 2);
    println!("{:?}", a);

    // A copy of a reference variable: held while the tuple (or struct)
    // is in use, not just where it's built.
    let r = &s2;
    let e = (Some(r), 3);
    let w = W { o: Some(r) };
    println!("{:?} {:?}", e, w);

    // A branch's own reference.
    let k = if n > 2 {
        let p = &s1;
        (Some(p), 4)
    } else {
        (Some(&s2), 5)
    };
    println!("{:?}", k);
    s1.push_str("!");
}
