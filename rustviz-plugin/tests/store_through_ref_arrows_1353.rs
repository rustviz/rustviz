// A call that may store a reference in the reference variable r through
// `&mut r` (`dst: &mut &'a String`) draws the borrow into r and its
// return from r at r's last use, as `r = &b;` does (#1353). Storing into
// a container (`keep(&mut v, &b)`, #513) stays drawn on b's column only.
fn store_s<'a>(dst: &mut &'a String, src: &'a String) { *dst = src; }
fn store_m<'a>(dst: &mut &'a mut String, src: &'a mut String) { *dst = src; }
fn keep<'a>(v: &mut Vec<&'a String>, s: &'a String) { v.push(s); }
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let mut r = &a;
    store_s(&mut r, &b);
    let _spacer = 1;
    println!("{}", r);
    let mut c = String::from("c");
    let mut d = String::from("d");
    let mut m = &mut c;
    store_m(&mut m, &mut d);
    m.push('!');
    let e = String::from("e");
    let mut v = Vec::new();
    keep(&mut v, &e);
    println!("{}", v.len());
}
