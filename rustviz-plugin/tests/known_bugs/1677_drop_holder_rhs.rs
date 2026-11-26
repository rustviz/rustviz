// Known bug, from the closed 2.x fix PRs (cx/oracle3-1677-1680-1683-1686); kept as a RustViz 3 regression case.
// Issue(s): #1677 (see known_bugs/README.md for what main draws wrong).
struct H<'a>(&'a String);
impl Drop for H<'_> {
    fn drop(&mut self) { println!("drop {}", self.0); }
}
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut h = H(&b);
    h = if c {
        let r = &a;
        H(r)
    } else {
        H(&a)
    };
    println!("{}", h.0);
}

// A plain borrowed field can stop borrowing before the assignment starts.
fn no_destructor(c: bool, old: &mut String, new: &String) {
    struct Plain<'a>(&'a String);
    let mut g = Plain(old);
    println!("{}", g.0);
    g = match c {
        true => {
            old.clear();
            Plain(new)
        }
        false => Plain(new),
    };
    println!("{}", g.0);
}

// The overwritten field and an independent reference can share a lender.
fn shared_old(c: bool) {
    let old = String::new();
    let new = String::new();
    let other = &old;
    let mut h = H(&old);
    h = if c {
        let r = &new;
        H(r)
    } else {
        H(&new)
    };
    println!("{}", other);
    println!("{}", h.0);
}

// The replacement and the old loan ending can share a row with the
// borrow of the incoming field (the value of the break).
fn loop_rhs(c: bool) {
    let old = String::new();
    let new = String::new();
    let mut h = H(&old);
    h = loop {
        if c { break H(&new); }
    };
    println!("{}", h.0);
}

// Review of #1777: these keep main's drop on the assignment's first row.
// A plain call spread over several lines.
fn multiline_call() {
    let old = String::new();
    let new = String::new();
    let mut h = H(&old);
    h = mk(
        &new,
    );
    println!("{}", h.0);
}
fn mk<'a>(s: &'a String) -> H<'a> { H(s) }

// A field write: p's lifetime keeps old borrowed while p is used.
struct P<'a> { h: H<'a> }
fn field_write(c: bool) {
    let old = String::new();
    let new = String::new();
    let mut p = P { h: H(&old) };
    p.h = if c {
        H(&new)
    } else {
        H(&new)
    };
    println!("{}", p.h.0);
}
