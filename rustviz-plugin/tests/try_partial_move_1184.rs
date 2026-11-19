// #1184: a `?` that returns early drops only what is left of a struct
// some fields of which were moved out before it, and nothing of it once
// every field that needs dropping is gone, as its scope end says.
struct P {
    a: String,
    b: String,
}

struct Q {
    a: String,
    n: i32,
}

fn bar() -> Option<i32> {
    Some(1)
}

fn both(_x: i32) -> Option<i32> {
    let p = P { a: String::from("a"), b: String::from("b") };
    drop(p.a);
    bar()?;
    drop(p.b);
    bar()?;
    Some(1)
}

// Moved on some paths only: the drop flag decides for that field.
fn maybe(c: bool) -> Option<i32> {
    let r = P { a: String::from("a"), b: String::from("b") };
    if c {
        drop(r.a);
    }
    bar()?;
    Some(1)
}

// The other field is Copy: nothing of q is left to drop.
fn copy_rest() -> Option<i32> {
    let q = Q { a: String::from("a"), n: 1 };
    drop(q.a);
    bar()?;
    Some(q.n)
}

fn main() {
    let _ = both(1);
    let _ = maybe(true);
    let _ = copy_rest();
}
