// #569: overwriting a whole struct with a call's result
// (`hold = make(&t);`) or with another variable (`hold = other;`) drops
// its old value, as a struct literal does (#538), so the borrow of s it
// held ends at that write (or at the old value's last use, if earlier).
// rustc agrees: the `let m = &mut s;` after the overwrite compiles. The
// struct's field now borrows what the new value borrows: `make`'s
// result borrows t (main drew it borrowing s again).
struct H<'a> { r: &'a String }
struct M<'a> { r: &'a mut String }
struct O<'a> { inner: H<'a>, n: i32 }

fn make<'a>(r: &'a String) -> H<'a> { H { r } }

impl<'a> M<'a> {
    fn new(r: &'a mut String) -> M<'a> { M { r } }
}

fn main() {
    let mut s = String::from("s");
    let t = String::from("t");
    let mut hold = H { r: &s };
    hold = make(&t);
    let m = &mut s;
    m.push('!');
    println!("{}", hold.r);
}

// Overwritten with another variable: a's borrow ends at the write too.
// b's borrow moves on to hold.r with the value (#637).
fn from_variable() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut hold = H { r: &a };
    let other = H { r: &b };
    hold = other;
    let m = &mut a;
    m.push('!');
    println!("{}", hold.r);
}

// The old value is used before it is overwritten: c's borrow ends at
// that last use.
fn used_before() {
    let c = String::from("c");
    let d = String::from("d");
    let mut hold = H { r: &c };
    println!("{}", hold.r);
    hold = make(&d);
    println!("{}", hold.r);
}

// A `&mut` field, overwritten with an associated function's result: u
// can be read before the last use of h.
fn mutable() {
    let mut u = String::from("u");
    let mut v = String::from("v");
    let mut h = M { r: &mut u };
    h.r.push('1');
    h = M::new(&mut v);
    println!("{}", u);
    h.r.push('2');
}

// The new value borrows what the old one did (`make(hold.r)` copies the
// reference): x stays borrowed until hold's last use, with one end.
fn same_lender() {
    let x = String::from("x");
    let mut hold = H { r: &x };
    hold = make(hold.r);
    println!("{}", hold.r);
}

// Overwriting a struct *field* of a struct is a partial write: `o` stays
// live, and its type has one lifetime for both references, so rustc
// keeps e borrowed until o's last use, as for `hold.r = &f;` (#531).
// (A `&mut e` between the write and the println! is E0502.) #568 drew
// e's borrow returned at the write.
fn field_struct() {
    let mut e = String::from("e");
    let f = String::from("f");
    let mut o = O { inner: H { r: &e }, n: 1 };
    o.inner = H { r: &f };
    println!("{} {}", o.inner.r, o.n);
    e.push('!');
}
