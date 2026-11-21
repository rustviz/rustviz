// #1148: a `move` closure that captures a `&mut` holds the borrow and
// owns nothing to drop; one that captures a String inside an `if` or a
// `match` arm drops it when it goes out of scope there. A captured
// value with nothing to drop isn't a resource either. #1159: a closure
// moved into another variable inside an `if` hands its captures over.
struct Plain { a: i32 }
fn mut_ref() {
    let mut s = String::from("hello");
    {
        let r = &mut s;
        let mut f = move || r.push('!');
        f();
    }
    println!("{}", s);
}
fn in_if(c: bool) {
    let t = String::from("hello");
    if c {
        let g = move || println!("{}", t);
        g();
    }
}
fn in_match(k: i32) {
    let u = String::from("hello");
    match k {
        0 => {
            let h = move || println!("{}", u);
            h();
        }
        _ => {}
    }
}
fn plain() {
    let p = Plain { a: 1 };
    let k = move || { let q = &p; q.a };
    k();
}
fn handed_over(c: bool) {
    let w = String::from("a");
    let m = move || println!("{}", w);
    if c {
        let n = m;
        n();
    }
}
fn main() {
    mut_ref();
    in_if(true);
    in_match(0);
    plain();
    handed_over(true);
}
