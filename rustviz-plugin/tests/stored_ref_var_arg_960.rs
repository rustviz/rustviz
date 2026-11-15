// #960: `keep(&mut v, &x)` with x a reference variable, where keep's
// signature lets it store `&x` in v: x itself stays borrowed while v is
// in use (rustc rejects `x = &b;` before `v.len()`), not just until
// keep returns.
fn keep<'a>(hold: &mut Vec<&'a &'a String>, r: &'a &'a String) {
    hold.push(r);
}

fn keep_mut<'a>(hold: &mut Vec<&'a mut &'a mut String>, r: &'a mut &'a mut String) {
    hold.push(r);
}

// Can't store its argument: the borrow of x ends at the call.
fn look(r: &&String) -> usize {
    r.len()
}

fn main() {
    let a = String::from("a");
    let x = &a;
    let n = look(&x);
    let mut v = Vec::new();
    keep(&mut v, &x);
    println!("{} {}", v.len(), n);

    let mut b = String::from("b");
    let mut m = &mut b;
    let mut w = Vec::new();
    keep_mut(&mut w, &mut m);
    println!("{}", w.len());
}
