// A `let` that takes a constructor apart (`let W(y) = W(g);`) is drawn
// like one that takes a tuple literal apart (`let (y, n) = (g, 1);`):
// the constructor is no actor (#950, see #873), and g's String goes to
// y once. What the pattern leaves (`_`, `..`) is moved into the
// temporary the constructor builds, and dropped with it at the end of
// the statement. A call that only returns a `W` (`wrap(m)`) is no
// constructor: v takes what it returns.
struct W(String);
struct W2(String, String);

fn wrap(s: String) -> W {
    W(s)
}

fn unwrap(g: String) {
    let W(y) = W(g);
    drop(y);
}

fn ignore_second(a: String, b: String) {
    let W2(p, _) = W2(a, b);
    drop(p);
}

fn rest(c: String, d: String) {
    let W2(q, ..) = W2(c, d);
    drop(q);
}

fn nested(e: String) {
    let (W(r), n) = (W(e), 1);
    drop(r);
    println!("{}", n);
}

fn let_else(h: String) {
    let Some(t) = Some(h) else { return; };
    drop(t);
}

fn assign(k: String) {
    let u;
    W(u) = W(k);
    drop(u);
}

fn by_call(m: String) {
    let W(v) = wrap(m);
    drop(v);
}

fn main() {
    unwrap(String::from("g"));
    ignore_second(String::from("a"), String::from("b"));
    rest(String::from("c"), String::from("d"));
    nested(String::from("e"));
    let_else(String::from("h"));
    assign(String::from("k"));
    by_call(String::from("m"));
}
