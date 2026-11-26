// #1483: a struct drawn with a column for its reference field, given a
// value its branch builds from a reference the branch declares (`h = if c
// { let r = &a; H(r) } else { .. }`). Each branch's value is paired with h
// in that branch, while `r` is still that branch's: h.0 borrows a or b.
// h's `impl Drop` reads h.0 when h is dropped, at main's end (line 37), so
// h.0 keeps that loan until then and gives a and b back there; each r's
// own borrow ends where `H(r)` copies it (Q7). The old value's loan of b ends where it
// is dropped, at the assignment. main drew h.0 borrowing b only, "Copy
// from r to H" next to "Copy from r to h.0", and r's borrow ending on line
// 37, after r was out of scope.
struct H<'a>(&'a String);
impl Drop for H<'_> {
    fn drop(&mut self) {
        println!("drop H({})", self.0);
    }
}

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut h = H(&b);
    println!("{}", h.0);
    h = if c {
        let r = &a;
        H(r)
    } else {
        let r = &b;
        H(r)
    };
    println!("{}", h.0);
    let_drop(c);
    no_drop(c);
    named(a.len());
    muts(c);
    copied(c);
}

// `let`: h2.0 keeps the branch's loan to h2's drop (main: r's end on the
// last line, after both scopes).
fn let_drop(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let h2 = if c {
        let r = &a;
        H(r)
    } else {
        let r = &b;
        H(r)
    };
    println!("{}", h2.0);
}

// No `impl Drop`: g.0's borrow ends at its last use. Its old loan of b
// ends at the old value's last use (line 62).
struct G<'a>(&'a String);

fn no_drop(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let mut g = G(&b);
    println!("{}", g.0);
    g = if c {
        let r = &a;
        G(r)
    } else {
        let r = &b;
        G(r)
    };
    println!("{}", g.0);
}

// A named field and a `match`.
struct N<'a> {
    p: &'a String,
}
impl Drop for N<'_> {
    fn drop(&mut self) {
        println!("drop {}", self.p);
    }
}

fn named(n: usize) {
    let a = String::from("a");
    let b = String::from("b");
    let mut k = N { p: &b };
    println!("{}", k.p);
    k = match n {
        0 => {
            let p = &a;
            N { p }
        }
        _ => {
            let q = &b;
            N { p: q }
        }
    };
    println!("{}", k.p);
}

// A `&mut` field: m.0 reborrows r, and gives a or b back at its last use,
// not "*r".
struct M<'a>(&'a mut String);

fn muts(c: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut z = String::from("z");
    let mut m = M(&mut z);
    m.0.push('!');
    m = if c {
        let r = &mut a;
        M(r)
    } else {
        let r = &mut b;
        M(r)
    };
    m.0.push('?');
    println!("{} {} {}", a, b, z);
}

// A Copy struct.
#[derive(Clone, Copy)]
struct K<'a>(&'a String);

fn copied(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let mut q = K(&b);
    println!("{}", q.0);
    q = if c {
        let r = &a;
        K(r)
    } else {
        let r = &b;
        K(r)
    };
    println!("{}", q.0);
}
