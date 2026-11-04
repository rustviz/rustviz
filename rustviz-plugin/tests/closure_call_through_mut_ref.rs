// The closure writes through a captured `&mut` reference (#133): it
// changes the String `r` refers to, not `r` itself.

fn main() {
    let mut s = String::new();
    let r = &mut s;
    let mut f = || r.push('a');
    f();
    r.push('b');
    println!("{}", s);
}
