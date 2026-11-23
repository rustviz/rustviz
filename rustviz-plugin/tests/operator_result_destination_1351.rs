// The value an overloaded operator returns goes where the expression is
// bound or returned, from the operator, as a call's result does (#1351).
// String's `+` takes `s` by value and returns it, so the result is s's
// own buffer moved on.
use std::ops::Neg;

struct Wave { samples: Vec<i32> }
impl Neg for Wave { type Output = Wave; fn neg(self) -> Wave { self } }

struct Tag { label: String }

fn take(s: String) { println!("{}", s); }

fn via_tail(s: String) -> String {
    s + "!"
}
fn via_let(s: String) -> String {
    let t = s + "!";
    t
}
fn via_return(s: String, c: bool) -> String {
    if c {
        return s + "?";
    }
    String::new()
}
fn via_assign(s: String) -> String {
    let mut x = String::new();
    x = s + "!";
    x
}
fn via_branch(s: String, c: bool) -> String {
    if c { s + "a" } else { s }
}
fn via_field(s: String) -> Tag {
    Tag { label: s + "!" }
}
fn via_arg(s: String) {
    take(s + "!");
}
// `x + 1` on `&i32` calls `<&i32 as Add<i32>>::add`, but its result is a
// plain number: no arrow from `+`.
fn plus_one(x: &i32) -> i32 {
    x + 1
}
fn via_add_assign(mut s: String) -> String {
    s += "!";
    s
}
fn main() {
    let s1 = String::from("a");
    let s2 = String::from("b");
    let s3 = s1 + &s2;
    let wave = Wave { samples: vec![1] };
    let flipped = -wave;
    println!("{} {} {}", s2, s3, flipped.samples.len());
}
