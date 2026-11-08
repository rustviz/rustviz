// A loan handed over to another reference ends where rustc's borrow
// checker ends it (#664, after #644). `o`'s type has one lifetime, so a
// loan that flowed into `o` stays live while `o` is, even after `o` got
// a new value. In each function the lender is still borrowed on the line
// between the two last uses (a use of it there is an error in rustc),
// and is used again after them.

fn id(r: &String) -> &String {
    r
}

// o got its reference from a call; h copies it, o gets a new one.
fn call_result() {
    let mut p = String::from("p");
    let q = String::from("q");
    let mut o = id(&p);
    let h = o;
    o = id(&q);
    println!("{}", h);
    println!("call {}", o);
    p.push('!');
}

// The same with a method call.
fn method_result() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut o = a.as_str();
    let h = o;
    o = b.as_str();
    println!("{}", h);
    println!("method {}", o);
    a.push('!');
}

// o gets its new value through w: w borrows o, so o stays live across
// the write, and the old loan with it.
fn write_through_call() {
    let mut c = String::from("c");
    let d = String::from("d");
    let mut o = id(&c);
    let h = o;
    let w = &mut o;
    *w = &d;
    println!("{}", h);
    println!("write call {}", o);
    c.push('!');
}

// The same with o made by `&x`.
fn write_through_borrow() {
    let mut x = String::from("x");
    let y = String::from("y");
    let mut o = &x;
    let h = o;
    let w = &mut o;
    *w = &y;
    println!("{}", h);
    println!("write borrow {}", o);
    x.push('!');
}

// No copy: x stays borrowed until o's last use, though o points to y
// from the write on.
fn write_through_only() {
    let mut s = String::from("s");
    let t = String::from("t");
    let mut o = &s;
    println!("{}", o);
    let w = &mut o;
    *w = &t;
    println!("write only {}", o);
    s.push('!');
}

fn main() {
    call_result();
    method_result();
    write_through_call();
    write_through_borrow();
    write_through_only();
}
