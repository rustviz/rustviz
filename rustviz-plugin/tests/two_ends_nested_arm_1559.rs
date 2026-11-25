// #1559: two loans of s (h.r and r) last used on a nested arm's line.
struct Holder<'a> {
    r: &'a String,
    n: i32,
}
fn else_if_chain() {
    let s = String::from("abc");
    let h = Holder { r: &s, n: 0 };
    let r = &s;
    if s.len() > 5 {
    } else if s.len() > 2 {
        println!("{} {}", r, h.n);
    } else {
    }
}
fn nested_if_in_else() {
    let s = String::from("abc");
    let h = Holder { r: &s, n: 0 };
    let r = &s;
    if s.len() > 5 {
    } else {
        if s.len() > 2 {
            println!("{} {}", r, h.n);
        } else {
        }
    }
}
fn match_in_arm(n: u8) {
    let s = String::from("abc");
    let h = Holder { r: &s, n: 0 };
    let r = &s;
    if s.len() > 5 {
    } else {
        match n {
            0 => println!("{} {}", r, h.n),
            _ => {}
        }
    }
}
fn else_if_let(o: Option<i32>) {
    let s = String::from("abc");
    let h = Holder { r: &s, n: 0 };
    let r = &s;
    if s.len() > 5 {
    } else if let Some(k) = o {
        println!("{} {} {}", r, h.n, k);
    } else {
    }
}
fn two_refs_else_if() {
    let s = String::from("abc");
    let a = &s;
    let r = &s;
    if s.len() > 5 {
    } else if s.len() > 2 {
        println!("{} {}", r, a);
    } else {
    }
}
fn in_loop() {
    let s = String::from("abc");
    let h = Holder { r: &s, n: 0 };
    let r = &s;
    for i in 0..2 {
        if i > 5 {
        } else if i > 0 {
            println!("{} {}", r, h.n);
        } else {
        }
    }
}
fn main() {
    else_if_chain();
    nested_if_in_else();
    match_in_arm(0);
    else_if_let(None);
    two_refs_else_if();
    in_loop();
}
