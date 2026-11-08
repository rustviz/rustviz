// #663: an arm that returns without using `r`. On that path r's borrow
// ended before the arm, so it ends where the arm starts, before the
// `return` drops `t`.
fn early(d: bool) -> usize {
    let t = String::from("t");
    let r = &t;
    if d {
        println!("early");
        return 0;
    } else {
        println!("late");
    }
    r.len()
}

// A match arm that returns.
fn arm(o: Option<usize>) -> usize {
    let s = String::from("s");
    let w = &s;
    let n = match o {
        Some(n) => n,
        None => return w.len(),
    };
    w.len() + n
}

// Two references to two lenders, both ending at the `return`.
fn two(d: bool) -> usize {
    let a = String::from("a");
    let b = String::from("b");
    let ra = &a;
    let rb = &b;
    if d {
        return ra.len() + rb.len();
    } else {
        println!("d");
    }
    ra.len() + rb.len()
}

fn main() {
    println!("{} {} {}", early(false), arm(Some(1)), two(true));
}
