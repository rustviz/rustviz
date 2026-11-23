// #1280: `break l.name` carries a field of a loop-local struct out of the
// loop. Only the iteration that breaks moves it; every other iteration
// reaches the end of the body and drops all of l there. The same for a
// whole struct with field columns, a tuple field, a nested field, a
// `break` in a match arm, after another field was moved, as part of a
// tuple, a loop the function returns, and a field of a Box.
struct Person { name: String, bio: String }
struct In { first: String, tag: String }
struct Out { inner: In, note: String }

fn field(c: bool) -> String {
    let r = loop {
        let l = Person { name: String::from("a"), bio: String::from("b") };
        if c { break l.name; }
    };
    r
}

fn whole(c: bool) -> Person {
    let r = loop {
        let k = Person { name: String::from("a"), bio: String::from("b") };
        if c { break k; }
    };
    println!("{}", r.name);
    r
}

fn tuple_field(c: bool) -> String {
    let r = loop {
        let t = (String::from("a"), String::from("b"));
        if c { break t.0; }
    };
    r
}

fn nested(c: bool) -> String {
    let r = loop {
        let o = Out { inner: In { first: String::from("a"), tag: String::from("t") }, note: String::from("n") };
        if c { break o.inner.first; }
    };
    r
}

fn in_match(n: i32) -> String {
    let r = loop {
        let m = Person { name: String::from("a"), bio: String::from("b") };
        match n {
            0 => break m.name,
            _ => {}
        }
    };
    r
}

fn after_partial(c: bool) -> String {
    let r = loop {
        let p = Person { name: String::from("a"), bio: String::from("b") };
        let b = p.bio;
        if c { break p.name; }
        drop(b);
    };
    r
}

fn pair(c: bool) -> (String, String) {
    let r = loop {
        let q = Person { name: String::from("a"), bio: String::from("b") };
        let j = String::from("j");
        if c { break (q.name, j); }
    };
    r
}

fn returned(c: bool) -> String {
    loop {
        let v = Person { name: String::from("a"), bio: String::from("b") };
        if c { break v.name; }
    }
}

fn boxed(c: bool) -> String {
    let r = loop {
        let b = Box::new(Person { name: String::from("a"), bio: String::from("b") });
        if c { break b.name; }
    };
    r
}

fn main() {
    let _ = field(true);
    let _ = whole(true);
    let _ = tuple_field(true);
    let _ = nested(true);
    let _ = in_match(0);
    let _ = after_partial(true);
    let _ = pair(true);
    let _ = returned(true);
    let _ = boxed(true);
}
