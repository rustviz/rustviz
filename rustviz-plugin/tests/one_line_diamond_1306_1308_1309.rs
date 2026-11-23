// The one-row diamond of a one-line conditional (#117), as in the Book's
// `longest` (§10.3). #1308: a reference parameter's dashed bracket ran
// on past the fork dot into the middle of the diamond. #1309: a return
// to the caller from the left arm was an L drawn into the diamond,
// across the other leg, ending under the fork. #1306: a one-line
// conditional nested in an arm of another one (`else if` on the line)
// was a one-row diamond inside the outer one, its fork level with or
// above the outer fork, and the outer leg ended in its middle.
fn take(s: String) { println!("{}", s); }
fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() { x } else { y }
}
fn longest_string<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() >= b.len() { a } else { b }
}
fn match_return<'a>(n: u8, x: &'a str, y: &'a str) -> &'a str {
    match n { 0 => x, _ => y }
}
fn owned_return(c: bool, s: String, t: String) -> String {
    if c { s } else { t }
}
fn early_return(flag: bool, s: String) -> String {
    if flag {
        return s;
    }
    String::from("other")
}
fn chain_return<'a>(c: bool, d: bool, x: &'a str, y: &'a str, z: &'a str) -> &'a str {
    if c { x } else if d { y } else { z }
}
fn nested_return<'a>(c: bool, d: bool, x: &'a str, y: &'a str, z: &'a str) -> &'a str {
    if c { if d { x } else { y } } else { z }
}
fn chain_calls(c: bool, d: bool, f: String, g: String, h: String) {
    if c { take(f) } else if d { take(g) } else { take(h) }
}
fn nested_calls(c: bool, d: bool, f: String, g: String, h: String) {
    if c { if d { take(f) } else { take(g) } } else { take(h) }
}
fn first_empty(c: bool, d: bool, g: String, h: String) {
    if c { } else if d { take(g) } else { take(h) }
}
fn match_in_else(c: bool, n: u8, f: String, g: String, h: String) {
    if c { take(f) } else { match n { 0 => take(g), _ => take(h) } }
}
fn chain_apart(c: bool, d: bool, f: String, mut a: String, g: String, mut b: String, h: String, mut e: String) {
    println!("{} {} {}", a, b, e);
    if c { a = f } else if d { b = g } else { e = h }
    println!("{} {} {}", a, b, e);
}
fn in_loop(v: Vec<(String, String, String)>, c: bool, d: bool) {
    for (f, g, h) in v {
        if c { take(f) } else if d { take(g) } else { take(h) }
    }
}
fn if_let_bracket(o: &Option<String>) {
    if let Some(s) = o {
        println!("{}", s);
    }
}
// #1309, from the stage-2 audit: a left arm's return L going left met a
// call's result to the caller in the gutter (heads 2px apart), and a
// call's logo on an arm's diagonal sat inside the diamond, on the other
// arm's leg.
fn unbox_or_new(b: Box<String>, c: bool) -> String {
    if c { *b } else { String::new() }
}
fn take_each(c: bool, s: String, t: String) {
    if c { take(s) } else { take(t) }
}
fn made_or_kept(c: bool, s: String) -> String {
    let x = if c { String::new() } else { s };
    x
}
fn pair_made(c: bool, f: String, h: String, k: String) {
    let (a, b) = if c { (f, String::new()) } else { (h, k) };
    println!("{} {}", a, b);
}
fn deferred(c: bool) {
    let s;
    if c {
        s = String::from("yes");
    } else {
        s = String::from("no");
    }
    println!("{}", s);
}
fn main() {
    let s1 = String::from("abcd");
    let r = longest(s1.as_str(), "xyz");
    println!("{}", r);
    let t1 = String::from("hello");
    let t2 = String::from("hi");
    println!("{}", longest_string(&t1, &t2));
    println!("{} {}", match_return(0, "a", "b"), owned_return(true, String::new(), String::new()));
    println!("{}", early_return(true, String::new()));
    println!("{} {}", chain_return(true, false, "a", "b", "c"), nested_return(true, false, "a", "b", "c"));
    chain_calls(true, false, String::new(), String::new(), String::new());
    nested_calls(true, false, String::new(), String::new(), String::new());
    first_empty(true, false, String::new(), String::new());
    match_in_else(true, 0, String::new(), String::new(), String::new());
    chain_apart(true, false, String::new(), String::new(), String::new(), String::new(), String::new(), String::new());
    in_loop(Vec::new(), true, false);
    if_let_bracket(&Some(String::new()));
    println!("{} {}", unbox_or_new(Box::new(String::new()), true), made_or_kept(true, String::new()));
    take_each(true, String::new(), String::new());
    pair_made(true, String::new(), String::new(), String::new());
    deferred(true);
}
