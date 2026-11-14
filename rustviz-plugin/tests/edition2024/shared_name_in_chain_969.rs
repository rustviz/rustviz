// #969: two block locals named `w`, each in a let-chain condition, are
// never in scope together, so they share a column. Each `w` is moved into
// `drop`, which drops its String, so nothing is dropped when it goes out
// of scope, and nothing about it depends on a branch or a drop flag; the
// same as with a name of their own (`w` and `w2`).

fn main() {
    let o = Some(3);
    let s = String::from("s");
    if let Some(x) = o && { let w = s; drop(w); x > 1 } && x > 5 {
        println!("y");
    }
    let t = String::from("t");
    if let Some(x) = o && { let w = t; drop(w); x > 1 } && x > 5 {
        println!("y");
    }
}

// The moving block is the chain's last part.
fn last_part(o: Option<i32>) {
    let a = String::from("a");
    if let Some(x) = o && { let w = a; drop(w); x > 1 } {
        println!("{}", x);
    }
    let b = String::from("b");
    if let Some(x) = o && { let w = b; drop(w); x > 1 } {
        println!("{}", x);
    }
}
