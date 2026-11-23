// `let t = Some(&String::from("b"));` / `[&make_user().name]`: the `let`
// extends the temporary to the end of the block (it has its own column,
// #856), and the value holds a borrow of it, drawn as for `Some(&s)`. It
// used to draw no borrow at all between the temporary and the holder
// (#1357).
#[derive(Debug)]
struct User { name: String, age: u32 }
fn make_user() -> User { User { name: String::from("ann"), age: 3 } }
fn lets() {
    let s = String::from("a");
    let o = Some(&s);
    let t = Some(&String::from("b"));
    let u = Some(&make_user().name);
    let a = [&make_user().name];
    println!("{:?} {:?} {:?} {:?}", o, t, u, a);
}
fn mixed() {
    let s = String::from("a");
    let a = [&String::from("x"), &s];
    println!("{:?}", a);
}
fn nested() {
    let o = Some(Some(&String::from("x")));
    let a = [Some(&make_user().name)];
    println!("{:?} {:?}", o, a);
}
fn mutable() {
    let m = Some(&mut make_user().age);
    if let Some(x) = m {
        *x += 1;
    }
}
fn ok() {
    let r: Result<&String, ()> = Ok(&String::from("r"));
    println!("{:?}", r);
}
fn pattern() {
    let m = Some(&String::from("x"));
    if let Some(x) = m {
        println!("{}", x);
    }
}
fn unwrapped() {
    let m = Some(&String::from("y"));
    let y = m.unwrap();
    println!("{}", y);
}
fn pattern_mut() {
    let m = Some(&mut String::from("z"));
    if let Some(x) = m {
        x.push('a');
    }
}
fn main() {
    lets();
    pattern();
    unwrapped();
    pattern_mut();
    mixed();
    nested();
    mutable();
    ok();
}
