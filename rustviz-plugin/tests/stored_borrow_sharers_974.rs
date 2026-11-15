// #974: after `keep(&mut v, &s)` may store `&s` in v, s stays shared
// until v's last use (`v.len()`), so a later shared borrow that ends
// before it doesn't end s's borrow: rustc rejects moving s
// (`let s2 = s;`) anywhere before `v.len()`. Likewise for t, stored in
// u, while q still borrows it after u's last use, and for a reference
// variable x stored by `keep_ref(&mut w, &x)`.
fn keep<'a>(hold: &mut Vec<&'a String>, r: &'a String) {
    hold.push(r);
}

fn keep_ref<'a>(hold: &mut Vec<&'a &'a String>, r: &'a &'a String) {
    hold.push(r);
}

fn look(r: &String) -> usize {
    r.len()
}

fn look_ref(r: &&String) -> usize {
    r.len()
}

fn main() {
    let s = String::from("s");
    let mut v = Vec::new();
    keep(&mut v, &s);
    let n = look(&s);
    let r = &s;
    println!("{} {}", r, n);
    println!("{}", v.len());

    let t = String::from("t");
    let mut u = Vec::new();
    keep(&mut u, &t);
    let q = &t;
    println!("{}", u.len());
    println!("{}", q);

    let a = String::from("a");
    let x = &a;
    let mut w = Vec::new();
    keep_ref(&mut w, &x);
    let k = look_ref(&x);
    println!("{} {}", w.len(), k);

    // The same for a reference that borrows y itself: ry still
    // borrows y after look_ref's borrow ends.
    let y = &a;
    let ry = &y;
    let j = look_ref(&y);
    println!("{} {}", ry.len(), j);
}
