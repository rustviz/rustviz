// #1505: `f(&b)` with `b: Box<String>` is deref-coerced to `&*b`, a borrow
// of the String inside b; the signature ties the result to it, so b stays
// borrowed until the result's last use.
struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn id<'a>(s: &'a String) -> &'a String {
    s
}

fn first(s: &str) -> &str {
    &s[..1]
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn calls() {
    let mut b = Box::new(String::from("hey"));
    let h = id(&b);
    let f = first(&b);
    println!("{h} {f}");
    let r3 = &b;
    let h2 = holder(r3);
    println!("{}", h2.r);
    b.push('!');
}

fn in_loop(n: i32) {
    let b = Box::new(String::from("hey"));
    for _ in 0..n {
        let h = holder(&b);
        println!("{}", h.r);
    }
}

fn in_let_else() {
    let b = Box::new(String::from("x"));
    let Some(x3) = maybe(String::from("rust")) else {
        let h = holder(&b);
        println!("{}", h.r);
        return;
    };
    println!("{x3}");
}

fn main() {
    calls();
    in_loop(1);
    in_let_else();
}
