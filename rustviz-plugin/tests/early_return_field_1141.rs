// #1141: `return p.name;` moves only p.name to the caller; what else p
// holds (p.bio) is dropped where the function returns.
struct Person { name: String, bio: String }
struct In { first: String, tag: String }
struct Out { inner: In, note: String }

fn in_if(c: bool) -> String {
    let p = Person { name: String::from("a"), bio: String::from("b") };
    if c { return p.name; }
    p.bio
}

fn in_match(n: i32) -> String {
    let q = Person { name: String::from("a"), bio: String::from("b") };
    match n {
        0 => return q.name,
        _ => {}
    }
    q.bio
}

fn tuple_field(c: bool) -> String {
    let t = (String::from("a"), String::from("b"));
    if c { return t.0; }
    t.1
}

fn nested(c: bool) -> String {
    let o = Out { inner: In { first: String::from("a"), tag: String::from("t") }, note: String::from("n") };
    if c { return o.inner.first; }
    o.note
}

fn wrapped(c: bool) -> Option<String> {
    let w = Person { name: String::from("a"), bio: String::from("b") };
    if c { return Some(w.name); }
    None
}

fn either(c: bool, d: bool) -> String {
    let e = Person { name: String::from("a"), bio: String::from("b") };
    if c {
        return if d { e.name } else { e.bio };
    }
    String::new()
}

fn boxed(c: bool) -> String {
    let b = Box::new(Person { name: String::from("a"), bio: String::from("b") });
    if c { return b.name; }
    String::new()
}

fn main() {
    let _ = in_if(true);
    let _ = in_match(0);
    let _ = tuple_field(true);
    let _ = nested(true);
    let _ = wrapped(true);
    let _ = either(true, true);
    let _ = boxed(true);
    let r = loop {
        let l = Person { name: String::from("a"), bio: String::from("b") };
        if l.bio.len() > 0 { break l.name; }
    };
    println!("{}", r);
}
