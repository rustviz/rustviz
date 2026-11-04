// #282: storing a borrow into an element (`v[0] = &u`) or another
// place with no column of its own names the element as the borrower
// (not "Anonymous resource"), and the borrow ends where the borrow
// checker ends it: at the last use of `v` / `w` (lines 17 and 22).
// After that, `c` can be used again and still owns its String.
struct Holder<'a> { r: &'a String }

fn point<'a>(h: &mut Holder<'a>, y: &'a String) {
    h.r = &y;
}

fn main() {
    let s = String::from("a");
    let u = String::from("b");
    let mut v: Vec<&String> = vec![&s];
    v[0] = &u;
    println!("{:?}", v);
    let mut c0 = String::new();
    let mut c = String::from("c");
    let mut w: Vec<&mut String> = vec![&mut c0];
    w[0] = &mut c;
    w[0].push('!');
    println!("{}", c);
    let mut h = Holder { r: &s };
    point(&mut h, &u);
    println!("{}", h.r);
}
