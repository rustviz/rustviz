// #1182: a reborrow made through a reference (`&*r`, `&r[..1]`, `&r.name`)
// borrows what r pointed to *then*. Giving r a new reference afterwards
// doesn't end that borrow: rustc kills the reborrow's own loan of `*r` at
// the write, but keeps r's old loan alive while the reborrow is used. So
// `a` stays borrowed until `first`'s last use, and `first` gives it back
// there. Main ended the reborrow, and freed `a`, at the reassignment, and
// (for a `&mut` r) lost r's line from there on (State::Invalid).
fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut r = &mut a;
    let first = &*r;
    r = &mut b;
    r.push('!');
    println!("{} {}", first, r);
}

// Shared: a slice of what r points to.
fn shared() {
    let c = String::from("c");
    let d = String::from("d");
    let mut q = &c;
    let part = &q[..1];
    q = &d;
    println!("{} {}", part, q);
}

// A field of what r points to.
struct P { name: String }

fn field() {
    let p = P { name: String::from("p") };
    let o = P { name: String::from("o") };
    let mut t = &p;
    let n = &t.name;
    t = &o;
    println!("{} {}", n, t.name);
}

// Two reborrows: the lender is given back at the later one's last use;
// the earlier one just ends.
fn two() {
    let e = String::from("e");
    let f = String::from("f");
    let mut u = &e;
    let one = &*u;
    let other = &u[1..];
    u = &f;
    println!("{}", other);
    println!("{} {}", one, u);
}

// The write in a branch.
fn branch(cond: bool) {
    let mut g = String::from("g");
    let mut h = String::from("h");
    let mut w = &mut g;
    let kept = &*w;
    if cond {
        w = &mut h;
    }
    println!("{} {}", kept, w);
}

// Control: the reborrow is dead before the write, so g2's borrow ends
// at its last use, as before.
fn dead() {
    let mut g2 = String::from("g2");
    let mut h2 = String::from("h2");
    let mut x = &mut g2;
    let early = &*x;
    println!("{}", early);
    x = &mut h2;
    x.push('!');
    println!("{}", g2);
}
