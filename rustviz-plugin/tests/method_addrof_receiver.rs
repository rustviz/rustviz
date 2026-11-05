// A written `&mut s` receiver is a fresh mutable borrow of the owner s,
// not a reborrow (#367): `(&mut s).push_str(..)` reads like
// `String::push_str(&mut s, ..)`. rustc does reborrow the temporary
// `&mut s` for the call, which made it look like a reborrow of s.
// `(&mut *r).push_str(..)` with r: `&mut String` is a reborrow of r,
// like `r.push_str(..)`.

fn main() {
    let mut s = String::from("hi");
    (&mut s).push_str("!");
    s.push_str("?");
    String::push_str(&mut s, ".");
    let r = &mut s;
    (&mut *r).push_str("a");
    r.push_str("b");
    println!("{}", s);
}
