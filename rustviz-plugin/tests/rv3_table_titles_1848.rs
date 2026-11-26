// RustViz 3 P1-2 review (#1848): a struct's line text as the state table
// words it.
struct Pair {
    x: String,
    y: String,
}

struct Outer {
    inner: Pair,
    note: String,
}

struct Boxed {
    inner: Box<Pair>,
    note: String,
}

// Each arm moves a different field of o.inner: every path leaves one of
// them, so o.inner wasn't moved out whole.
fn one_each(c: bool) {
    let o = Outer { inner: Pair { x: String::new(), y: String::new() }, note: String::new() };
    if c {
        drop(o.inner.x);
    } else {
        drop(o.inner.y);
    }
    println!("{}", o.note);
}

// Both fields of a boxed payload moved out: b.inner itself wasn't, its
// Box still owns the allocation.
fn boxed_payload() {
    let b = Boxed { inner: Box::new(Pair { x: String::new(), y: String::new() }), note: String::new() };
    drop(b.inner.x);
    drop(b.inner.y);
    println!("{}", b.note);
}

// A move in an else arm on the `} else {` row, then a comment row.
fn else_row(c: bool) {
    let p = Pair { x: String::new(), y: String::new() };
    if c {
        println!("{}", p.x);
    } else { drop(p.x); }
    // p.x may be gone here
    println!("{}", p.y);
}

// An array element moved out of a field: it has no column of its own.
struct Arr {
    a: [String; 2],
    q: Box<String>,
    z: String,
}

fn elem(p: Arr) {
    let [x, _] = p.a;
    drop(*p.q);
    println!("{} {}", x, p.z);
}

// One field moved out, a sibling only on some paths, a third whole: the
// line doesn't say the fields left can be used (t.b may be gone).
struct Three {
    a: String,
    b: String,
    c: String,
}

fn sibling_maybe(t: Three, k: bool) {
    let x = t.a;
    if k {
        drop(t.b);
    }
    println!("{} {}", x, t.c);
}

// A sibling mutably borrowed: not usable until the borrow ends.
fn sibling_lent(mut t: Three) {
    let x = t.a;
    let r = &mut t.b;
    r.push('!');
    println!("{} {}", x, t.c);
}

fn main() {
    one_each(true);
    boxed_payload();
    else_row(false);
    sibling_maybe(Three { a: String::new(), b: String::new(), c: String::new() }, true);
    sibling_lent(Three { a: String::new(), b: String::new(), c: String::new() });
    elem(Arr { a: [String::new(), String::new()], q: Box::new(String::new()), z: String::new() });
}
