// #1529: two borrows of one variable on one line, the first ending before
// the second starts. The first loan ends between the two borrows: s is
// never borrowed by both at once.
fn two_mut() {
    let mut s = String::from("a");
    let a = &mut s; let b = &mut s;
}

fn shared_then_mut() {
    let mut s = String::from("a");
    let r = &s; let m = &mut s;
    println!("{}", s);
}

fn mut_then_shared() {
    let mut s = String::from("a");
    let m = &mut s; let r = &s;
    println!("{}", s);
}

fn mut_used_later() {
    let mut s = String::from("a");
    let x = &s; let m = &mut s;
    m.push('b');
}

fn ends_in_condition() {
    let mut s = String::new();
    let r = &s;
    if r.len() > 1 { let m = &mut s; }
}

fn if_let_get_last_mut() {
    let mut v = vec![String::from("a")];
    if let Some(x2) = v.get(0) { let m2 = v.last_mut().unwrap(); }
    drop(v);
}

fn if_let_first_index_mut() {
    let mut v: Vec<String> = vec![String::from("a")];
    if let Some(x) = v.first() { let m = &mut v[0]; }
    let t = v.remove(0);
}

fn in_loop_and_arm(n: i32) {
    let mut s = String::from("a");
    for _ in 0..2 {
        let a = &mut s; let b = &mut s;
    }
    match n {
        1 => { let r = &s; let m = &mut s; }
        _ => {}
    }
    println!("{}", s);
}

fn main() {
    two_mut();
    shared_then_mut();
    mut_then_shared();
    mut_used_later();
    ends_in_condition();
    if_let_get_last_mut();
    if_let_first_index_mut();
    in_loop_and_arm(1);
}
