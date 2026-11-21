fn store<'a>(dst: &mut &'a str, src: &'a str) { *dst = src; }
fn store_s<'a>(dst: &mut &'a String, src: &'a String) { *dst = src; }
fn by_ref() {
    let a = String::from("a");
    let b1 = String::from("b");
    let mut r1 = &a;
    store_s(&mut r1, &b1);
    let _spacer = 1;
    println!("{}", r1);
}
fn by_call() {
    let a = String::from("a");
    let b2 = String::from("b");
    let mut r2 = a.as_str();
    store(&mut r2, b2.as_str());
    let _spacer = 1;
    println!("{}", r2);
}
fn by_coerce() {
    let a = String::from("a");
    let b3 = String::from("b");
    let mut r3 = a.as_str();
    store(&mut r3, &b3);
    let _spacer = 1;
    println!("{}", r3);
}
fn replace_in_branch(c: bool) {
    let a = String::from("a");
    let b4 = String::from("b");
    let mut r4 = &a;
    if c {
        std::mem::replace(&mut r4, &b4);
    }
    let _spacer = 1;
    println!("{}", r4);
}
fn through_mut_ref() {
    let a = String::from("a");
    let b5 = String::from("b");
    let mut r5 = &a;
    let rr = &mut r5;
    store_s(rr, &b5);
    let _spacer = 1;
    println!("{}", r5);
}
fn replace_kept() {
    let a = String::from("a");
    let b6 = String::from("b");
    let mut r6 = &a;
    let _old = std::mem::replace(&mut r6, &b6);
    let _spacer = 1;
    println!("{}", r6);
}
fn main() { by_ref(); by_call(); by_coerce(); replace_in_branch(true); through_mut_ref(); replace_kept(); }
