// #1419: a closure that reborrows through a reference parameter (`|| r.len()`)
// or copies it (`move || r.len()`) keeps r's borrow from the caller live
// until the closure's last use. It doesn't end at r's last use outside the
// closure, in a let-chain or anywhere else (rustc records no loan for a
// reborrow through a shared reference, so the closure's own liveness says
// it).
fn let_chain(r: &String) -> usize {
    let o = Some(String::from("x"));
    if let Some(x) = o && r.is_empty() {
        println!("{}", x);
    }
    let c = || r.len();
    c()
}

fn plain(r: &String) -> usize {
    let n = r.len();
    let c = || r.len();
    c() + n
}

fn moved_copy(r: &String) -> usize {
    let o = Some(String::from("x"));
    if let Some(x) = o && r.is_empty() {
        println!("{}", x);
    }
    let c = move || r.len();
    c()
}

fn mutable(r: &mut String) -> usize {
    let o = Some(String::from("x"));
    if let Some(x) = o && r.is_empty() {
        println!("{}", x);
    }
    let mut c = || r.push('a');
    c();
    r.len()
}

fn main() {
    let mut s = String::from("a");
    let_chain(&s);
    plain(&s);
    moved_copy(&s);
    mutable(&mut s);
}
