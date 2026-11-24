// #1384: a reference copied into an element of a holder (`refs[0] = tr`,
// `*el = tr`) or stored through a call's result that points into it
// (`*refs.get_mut(0).unwrap() = &t1`) is held by refs, as
// `refs[0] = &t1` (#1361) and `refs.push(tr)` are: refs borrows t1 until
// its last use. A `&mut` tr is reborrowed (`&mut *tr`), not moved.
fn h1() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let tr = &t1;
    let mut refs = vec![&a1];
    refs[0] = tr;
    println!("{:?}", refs);
}

fn h2() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let mut refs = vec![&a1];
    *refs.get_mut(0).unwrap() = &t1;
    println!("{:?}", refs);
}

fn h3() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let tr = &t1;
    let mut refs = vec![&a1];
    let el = &mut refs[0];
    *el = tr;
    println!("{:?}", refs);
}

fn h4() {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let tr = &t1;
    let mut refs = vec![&a1];
    *refs.last_mut().unwrap() = tr;
    println!("{:?}", refs);
}

fn h5(c: bool) {
    let a1 = String::from("a1");
    let t1 = String::from("t1");
    let tr = &t1;
    let mut refs = vec![&a1];
    if c {
        refs[0] = tr;
    }
    println!("{:?}", refs);
}

fn h6() {
    let mut a1 = String::from("a1");
    let mut t1 = String::from("t1");
    let mut refs = vec![&mut a1];
    let tr = &mut t1;
    refs[0] = tr;
    refs[0].push('x');
}

fn main() {
    h1();
    h2();
    h3();
    h4();
    h5(true);
    h6();
}
