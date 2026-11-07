// #663: `r` borrows `t` before an `if` and is used again after it. On
// the path through the arm that returns, the `return` is r's last use:
// its borrow ends there, before the `return` drops `t`. The end drawn
// at the later `r.len()` is never reached on that path.
fn top(d: bool) -> usize {
    let t = String::from("t");
    let r = &t;
    if d {
        return r.len();
    } else {
        println!("d");
    }
    r.len()
}

// `q` declared in an outer branch, the `return` in a nested `if`.
fn nested(c: bool, d: bool) -> usize {
    let u = String::from("u");
    if c {
        let q = &u;
        if d {
            return q.len() + 1;
        } else {
            println!("d");
        }
        q.len()
    } else {
        0
    }
}

// A panic ends a `&mut` borrow the same way.
fn panics(d: bool) -> usize {
    let mut v = String::from("v");
    let m = &mut v;
    if d {
        panic!("{}", m);
    } else {
        m.push('x');
    }
    m.len()
}

fn main() {
    println!("{} {} {}", top(true), nested(true, false), panics(false));
}
