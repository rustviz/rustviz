// Follow-ups to #1599 (Codex review): a shared reference made from a
// mutable borrow (`let r: &String = &mut b;`) keeps that loan wherever it
// is stored or copied, but what holds it can only read; other lenders
// stored next to it keep their own kind, and what it points to (copied
// out through it) isn't under its loan.
fn first<'a>(v: &Vec<&'a String>) -> &'a String {
    v[0]
}

// One store, two kinds: a shared loan of a, b's mutable loan (a can be
// read while v is live; b can't be used).
fn tuple_store() {
    let a = String::from("a");
    let mut b = String::from("b");
    let mut v: Vec<(&String, &String)> = Vec::new();
    let r: &String = &mut b;
    v.push((&a, r));
    println!("{}", a);
    println!("{:?}", v);
}

// `first` copies out a shared reference stored in x: w borrows a
// immutably; x's mutable loan ends with rx (`x.clear()` is accepted).
fn copied_out() {
    let a2 = String::from("a");
    let mut x: Vec<&String> = vec![&a2];
    let mut w: Vec<&String> = Vec::new();
    let rx: &Vec<&String> = &mut x;
    w.push(first(rx));
    x.clear();
    println!("{}", a2);
    println!("{:?}", w);
}

// `&*rc` reborrows through rc and keeps its mutable loan: c can't even
// be read before u's last use (E0502).
fn explicit_reborrow() {
    let mut c = String::from("c");
    let mut u: Vec<&String> = Vec::new();
    let rc: &String = &mut c;
    u.push(&*rc);
    println!("{:?}", u);
}

// A branch's value: o keeps rd's mutable loan of d, and a shared loan of e.
fn in_branch(flag: bool) {
    let mut d = String::from("d");
    let e = String::from("e");
    let rd: &String = &mut d;
    let o = Some(if flag { rd } else { &e });
    println!("{:?}", o);
}

// r1, a copy of r0, stops being used; the mutable loan goes on in r0.
fn copy_used_after() {
    let mut f = String::from("f");
    let r0: &String = &mut f;
    {
        let r1 = r0;
        println!("{}", r1);
    }
    println!("{}", r0);
}

// Every copy in a long chain holds the same mutable loan.
fn long_chain(v: Vec<i32>) {
    let mut g = String::from("g");
    let q0: &String = &mut g;
    if v.is_empty() {
        let q1 = q0;
        let q2 = q1;
        let q3 = q2;
        let q4 = q3;
        let q5 = q4;
        let q6 = q5;
        let q7 = q6;
        println!("{}", q7);
    }
    println!("{}", q0);
}

fn main() {
    tuple_store();
    copied_out();
    explicit_reborrow();
    in_branch(true);
    copy_used_after();
    long_chain(vec![]);
}
