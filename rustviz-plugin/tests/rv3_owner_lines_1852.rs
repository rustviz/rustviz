// RustViz 3 P1-3 review (#1852): owner columns' lines from the state table.
mod m {
    pub struct S {
        pub a: String,
        b: String,
    }
    pub fn new() -> S {
        S { a: String::new(), b: String::new() }
    }
}

// A field private to another module has no column: moving the public one
// leaves the private one in it (dropped with it at its scope end).
fn private_field() {
    let s = m::new();
    drop(s.a);
    let p = Box::new(m::new());
    drop(p.a);
    println!("done");
}

// One arm moves both payload fields, the other neither.
fn boxed_maybe(c: bool) {
    let b = Box::new((String::new(), String::new()));
    if c {
        drop(b.0);
        drop(b.1);
    }
    println!("after");
}

// `&mut *b` borrows the whole payload, its fields too.
fn payload_borrowed() {
    let mut b = Box::new((String::new(), 1));
    let r = &mut *b;
    r.0.push('x');
    println!("{}", b.1);
}

// Each path lends x to a different holder.
fn two_holders(c: bool) {
    let mut x = String::new();
    let mut y = String::new();
    let (r, s) = if c { (&mut x, &mut y) } else { (&mut y, &mut x) };
    r.push('a');
    s.push('b');
    println!("{} {}", x, y);
}

// A zero-sized payload: moving it out of its Box frees no allocation.
struct Token;
impl Drop for Token {
    fn drop(&mut self) {}
}

fn zst() {
    let b = Box::new(Token);
    let t = *b;
    drop(t);
    println!("zst");
}

fn main() {
    private_field();
    boxed_maybe(true);
    payload_borrowed();
    two_holders(true);
    zst();
}
