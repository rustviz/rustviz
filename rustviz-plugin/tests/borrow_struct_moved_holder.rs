// #637: a struct whose reference field holds a borrow, moved to another
// variable, takes the borrow with it. The loan `other.r` was made with
// stays live while any value holding the reference is live, so t stays
// borrowed until hold's last use (the `t.push` after it compiles; one
// before it would be E0502), and hold.r is what gives t back. Main
// drew t's borrow returned from other.r and hold.r borrowing from
// something not shown.
struct H<'a> { r: &'a String }
struct M<'a> { r: &'a mut String }
struct O<'a> { inner: H<'a>, n: String }
#[derive(Clone, Copy)]
struct C<'a> { r: &'a String }

fn make<'a>(r: &'a String) -> H<'a> { H { r } }

fn main() {
    let mut t = String::from("t");
    let other = H { r: &t };
    let hold = other;
    println!("{}", hold.r);
    t.push('x');
}

// Assigned over an earlier value: a's borrow ends at the write (#569),
// b's moves on to hold and ends at hold's last use.
fn assigned() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut hold = H { r: &a };
    let other = H { r: &b };
    hold = other;
    a.push('!');
    println!("assigned {}", hold.r);
    b.push('!');
}

// A `&mut` field: u can be used again after hold's last use.
fn mutable() {
    let mut u = String::from("u");
    let other = M { r: &mut u };
    let hold = other;
    hold.r.push('1');
    u.push('2');
}

// A struct field of a struct that holds a borrow moves with it.
fn nested() {
    let mut e = String::from("e");
    let o = O { inner: H { r: &e }, n: String::from("n") };
    let o2 = o;
    println!("{} {}", o2.inner.r, o2.n);
    e.push('!');
}

// A struct a call returned, moved on.
fn from_call() {
    let mut f = String::from("f");
    let other = make(&f);
    let hold = other;
    println!("call {}", hold.r);
    f.push('!');
}

// A Copy struct is copied: both copies hold the borrow.
fn copied() {
    let mut g = String::from("g");
    let c1 = C { r: &g };
    let c2 = c1;
    println!("{} {}", c1.r, c2.r);
    g.push('!');
}

// The moved-from variable gets a new value: its new borrow (of q) is its
// own, and p's borrow went with the old value to hold, so the write
// ends nothing on other.r. (p stays borrowed until other's last use:
// `other`'s type has one lifetime, and the loan flowed into it.)
fn reassigned_source() {
    let mut p = String::from("p");
    let mut q = String::from("q");
    let mut other = H { r: &p };
    let hold = other;
    other = H { r: &q };
    println!("source {}", hold.r);
    println!("source {}", other.r);
    p.push('!');
    q.push('!');
}

// Moved inside an arm: on that path hold.r gives w back at its last use;
// on the other, other.r is never used again, so w's borrow ends as the
// arm starts. (Main drew the end on other.r in the if arm.)
fn in_branch() {
    let mut w = String::from("w");
    let other = H { r: &w };
    if w.len() > 1 {
        let hold = other;
        println!("branch {}", hold.r);
    }
    w.push('!');
}
