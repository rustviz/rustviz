// #1528: a closure capturing s in an outer arm, a nested if assigns s / calls the closure.
fn nested_if() {
    let c = true;
    let mut s = String::from("ab");
    let t = String::new();
    if c {
        let c2 = || s.len();
        if c {
            s = t;
        } else {
            c2();
        }
    }
}
fn nested_if_let() {
    let c = true;
    let o = Some(1);
    let mut s = String::from("ab");
    let t = String::new();
    if c {
        let c2 = || s.len();
        if let Some(_) = o {
            s = t;
        } else {
            c2();
        }
    }
}
fn else_if_arm() {
    let n = 3;
    if n == 1 {
    } else if n == 2 {
        let s = String::new();
        let c2 = || s.len();
        if n == 3 {
        } else {
            c2();
        }
    } else {
    }
}
fn main() {}
