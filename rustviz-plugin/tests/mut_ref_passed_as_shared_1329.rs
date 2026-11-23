// #1329: a `&mut T` used where a `&T` is expected is coerced by an
// implicit immutable reborrow: `look(r)` with `look(x: &i32)` is
// `look(&*r)`. r isn't moved: it is read-only while the reborrow is live
// and can be used (even written through) afterwards. Only a generic `T`
// parameter takes r itself, moving it.
fn look(x: &i32) -> bool { *x > 2 }
fn bump(x: &mut i32) { *x += 1; }
fn take<T>(_t: T) {}
struct C { n: i32 }
impl C {
    fn get(&self) -> i32 { self.n }
    fn add(&mut self, x: &i32) { self.n += *x; }
}
struct V<'a> { r: &'a i32 }

// A call argument, then a write through r.
fn call_arg() {
    let mut e = 0;
    let r = &mut e;
    look(r);
    *r += 1;
    println!("{}", e);
}

// A `&mut` parameter.
fn param(r: &mut i32) {
    look(r);
    *r += 1;
}

// In an `if` condition.
fn in_cond() {
    let mut e = 0;
    let r = &mut e;
    if look(r) {
        *r += 1;
    }
    println!("{}", e);
}

// The third arm of an `else if` chain (the #1326 sibling): both joins
// are drawn. (The legs around `look(r)`'s dot are #1338.)
fn else_if(a: bool, b: bool) {
    let mut e = 0;
    let r = &mut e;
    if a {
    } else if b {
        bump(r);
    } else if look(r) {
    } else {
    }
    println!("{}", e);
}

// A closure's `&i32` parameter, and a method's.
fn closure_and_method() {
    let check = |x: &i32| *x > 2;
    let mut c = C { n: 0 };
    let mut e = 5;
    let r = &mut e;
    check(r);
    c.add(r);
    *r += 1;
    println!("{} {}", c.n, e);
}

// A method receiver (`&self`): already a reborrow before #1329.
fn receiver() {
    let mut c = C { n: 1 };
    let r = &mut c;
    r.get();
    r.n += 1;
    println!("{}", c.n);
}

// Where the expected type is written: a `let`, an assignment, a struct
// field, `Some(..)` and a tuple.
fn expected_types() {
    let mut e = 0;
    let r = &mut e;
    let s: &i32 = r;
    println!("{}", s);
    let t: &i32;
    t = r;
    println!("{}", t);
    let v = V { r: r };
    println!("{}", v.r);
    let o: Option<&i32> = Some(r);
    println!("{:?}", o);
    let p: (&i32, i32) = (r, 1);
    println!("{:?}", p);
    *r += 1;
    println!("{}", e);
}

// Returned where a `&i32` is expected.
fn shared_view(r: &mut i32) -> &i32 {
    r
}

// A generic `T` parameter takes the reference itself: m is moved.
fn generic() {
    let mut e = 0;
    let m = &mut e;
    take(m);
    println!("{}", e);
}

// A call whose result keeps the reborrow: r is read-only while w is
// live, and usable again after.
fn first_shared(x: &String) -> &str { &x[..1] }
fn call_result() {
    let mut q = String::from("ab");
    let r = &mut q;
    let w = first_shared(r);
    println!("{}", w);
    r.push('x');
    println!("{}", q);
}

fn main() {
    let mut n = 1;
    param(&mut n);
    let _ = shared_view(&mut n);
    call_arg();
    in_cond();
    else_if(false, false);
    closure_and_method();
    receiver();
    expected_types();
    generic();
    call_result();
}
