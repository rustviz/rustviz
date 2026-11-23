// `first_shared(&*r)` with r a local `&mut`: the explicit reborrow `&*r`
// passed to a `&T` parameter, and every other borrow of a place reached
// through r (`(*r).as_str()`, `first_mut(&mut *r)`, `r.name.as_str()`,
// `first_shared(&r.name)`), borrows *through* r, as the implicit form
// `first_shared(r)` does (#1329): r is read-only (or unusable) until the
// result's last use, then usable again. They used to borrow r's lender s
// directly while r still held its `&mut`, and s's column reached
// State::Invalid (#1332).
struct U { name: String, n: i32 }
fn first_shared(x: &String) -> &str { &x[..1] }
fn first_mut(x: &mut String) -> &mut String { x }
fn call_arg() {
    let mut s = String::from("ab");
    let r = &mut s;
    let w = first_shared(&*r);
    println!("{}", w);
    r.push('x');
    println!("{}", s);
}
fn receiver() {
    let mut s = String::from("ab");
    let r = &mut s;
    let w = (*r).as_str();
    println!("{}", w);
    r.push('x');
    println!("{}", s);
}
fn mutable() {
    let mut s = String::from("ab");
    let r = &mut s;
    let w = first_mut(&mut *r);
    w.push('y');
    r.push('x');
    println!("{}", s);
}
fn in_some() {
    let mut s = String::from("ab");
    let r = &mut s;
    let o = Some(first_shared(&*r));
    println!("{:?}", o);
    r.push('x');
    println!("{}", s);
}
fn in_branch(c: bool) {
    let mut s = String::from("ab");
    let r = &mut s;
    let w = if c { first_shared(&*r) } else { "z" };
    println!("{}", w);
    r.push('x');
    println!("{}", s);
}
fn assigned() {
    let mut s = String::from("ab");
    let r = &mut s;
    let w;
    w = first_shared(&*r);
    println!("{}", w);
    r.push('x');
    println!("{}", s);
}
fn field_arg() {
    let mut u = U { name: String::from("a"), n: 1 };
    let r = &mut u;
    let w = first_shared(&r.name);
    println!("{}", w);
    r.n += 1;
    println!("{}", u.n);
}
fn field_receiver() {
    let mut u = U { name: String::from("a"), n: 1 };
    let r = &mut u;
    let w = r.name.as_str();
    println!("{}", w);
    r.n += 1;
    println!("{}", u.n);
}
fn main() {
    call_arg();
    receiver();
    mutable();
    in_some();
    in_branch(true);
    assigned();
    field_arg();
    field_receiver();
}
