// A borrow ends where rustc's borrow checker ends it (#644). A variable's
// type has one lifetime for the references it holds, so a loan that once
// flowed into it stays live while the variable is, even after the value
// holding the loan moved on to another variable. In each function the
// lender is still borrowed on the line between the two last uses (a use
// of it there is an error in rustc), and is used again after them.

struct H<'a> {
    r: &'a String,
}

// p's borrow went to hold with the struct; other gets a new value, but
// p stays borrowed until other's last use.
fn moved_struct() {
    let mut p = String::from("p");
    let q = String::from("q");
    let mut other = H { r: &p };
    let hold = other;
    other = H { r: &q };
    println!("{}", hold.r);
    println!("{}", other.r);
    p.push('!');
}

// The same with a plain reference, copied.
fn copied_ref() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut o = &a;
    let h = o;
    o = &b;
    println!("{}", h);
    println!("{}", o);
    a.push('!');
}

// And moved (`&mut`).
fn moved_mut_ref() {
    let mut c = String::from("c");
    let mut d = String::from("d");
    let mut o2 = &mut c;
    let h2 = o2;
    o2 = &mut d;
    h2.push('1');
    o2.push('2');
    c.push('!');
}

// `rb = &mut n;` makes rb's pointee type `&mut String` with one
// lifetime for m's and n's references: x stays borrowed until rb's last use.
fn tied_lifetimes() {
    let mut x = String::from("x");
    let mut z = String::from("z");
    let mut m = &mut x;
    let mut n = &mut z;
    let mut rb = &mut m;
    rb.push('0');
    rb = &mut n;
    println!("{}", m);
    println!("{}", rb);
    x.push('!');
}

// A swap ties m1's and m2's lifetimes: w stays borrowed until m2's last use.
fn swapped() {
    let mut u = String::from("u");
    let mut w = String::from("w");
    let mut m1 = &mut u;
    let mut m2 = &mut w;
    (m1, m2) = (m2, m1);
    m1.push('1');
    m2.push('2');
    u.push('!');
    w.push('!');
}

fn main() {
    moved_struct();
    copied_ref();
    moved_mut_ref();
    tied_lifetimes();
    swapped();
}
