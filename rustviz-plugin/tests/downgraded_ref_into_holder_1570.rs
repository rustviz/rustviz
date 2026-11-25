// #1570: a shared reference holding a mutable loan (`let r: &String =
// &mut s;`, `downgrade(&mut s)`) stored in a value that holds references
// (`vr.push(r)`, `vec![r.as_str()]`, `Some(r)`). The value keeps r's
// loan, which is mutable: s stays mutably borrowed until the holder's
// last use, and there is no separate shared loan of s.
fn downgrade(s: &mut String) -> &String {
    s
}

fn pushed() {
    let mut s = String::from("hi");
    let mut vr: Vec<&String> = vec![];
    let r: &String = &mut s;
    vr.push(r);
    println!("{:?}", vr);
}

fn pushed_str() {
    let mut s1 = String::from("hi");
    let mut vs: Vec<&str> = vec![];
    let r1: &String = &mut s1;
    vs.push(r1.as_str());
    println!("{:?}", vs);
}

fn vec_literal() {
    let mut s2 = String::from("hi");
    let r2: &String = &mut s2;
    let v2 = vec![r2.as_str()];
    println!("{:?}", v2);
}

fn downgraded_pushed() {
    let mut s3 = String::from("hi");
    let mut v3: Vec<&String> = vec![];
    let r3 = downgrade(&mut s3);
    v3.insert(0, r3);
    println!("{:?}", v3);
}

fn in_some() {
    let mut s4 = String::from("hi");
    let r4: &String = &mut s4;
    let o4 = Some(r4);
    println!("{:?}", o4);
}

fn assigned_some() {
    let mut s5 = String::from("hi");
    let mut o5: Option<&String> = None;
    let r5: &String = &mut s5;
    o5 = Some(r5);
    println!("{:?}", o5);
}

fn main() {
    pushed();
    pushed_str();
    vec_literal();
    downgraded_pushed();
    in_some();
    assigned_some();
}
