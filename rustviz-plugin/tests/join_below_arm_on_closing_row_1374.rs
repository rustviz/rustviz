fn take(_s: String) {}
fn take_len(s: String) -> usize { s.len() }
fn else_call() {
    let s = String::from("hi");
    let c = true;
    if c {
        println!("{}", s);
    } else { take(s); }
}
fn else_borrow() {
    let s = String::from("hi");
    let c = true;
    if c {
        println!("{}", s);
    } else { println!("{} {}", s, s.len()); }
    println!("{}", s);
}
fn else_method() {
    let mut s = String::from("hi");
    let c = true;
    if c {
        println!("{}", s);
    } else { s.push('!'); }
    println!("{}", s);
}
fn match_last_arm(n: u32) {
    let s = String::from("hi");
    match n {
        0 => println!("{}", s),
        _ => take(s) }
}
fn let_else_call(c: bool) -> usize {
    let s = String::from("hi");
    let n = if c {
        s.len()
    } else { take_len(s) };
    n
}
fn else_if_call(c: bool, d: bool) {
    let s = String::from("hi");
    if c {
        println!("{}", s);
    } else if d { take(s); }
}
fn else_move_to_var(c: bool) {
    let s = String::from("hi");
    if c {
        println!("{}", s);
    } else { let t = s; println!("{}", t); }
}
fn main() {
    else_call();
    else_borrow();
    else_method();
    match_last_arm(1);
    let_else_call(true);
    else_if_call(false, true);
    else_move_to_var(true);
}
