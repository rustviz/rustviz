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
