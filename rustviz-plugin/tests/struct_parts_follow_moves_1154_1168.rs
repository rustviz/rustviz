// A struct moved somewhere that has no column per field takes its fields
// with it: the temporary a `let`'s literal builds (#1154), or the place a
// reference points to (`*slot = s`, #1168). Its field columns used to
// stay owners and claim a second drop at scope end.
struct Q { s: String }
struct B { name: String }
struct O { inner: B, n: i32 }

fn mk() -> Q { Q { s: String::from("q") } }

// The tuple a `let` builds; `_` leaves q2 in it, dropped with it.
fn let_tuple() {
    let q2 = Q { s: String::from("z") };
    let (a, _) = (1, q2);
    println!("{}", a);
}

// Nothing binds the tuple at all.
fn let_wild() {
    let q3 = Q { s: String::from("y") };
    let _ = (q3, 5);
}

// A `ref` binding extends the temporary to the end of the block.
fn let_ref() {
    let q4 = mk();
    let (n4, ref r4) = (1, q4);
    println!("{} {}", n4, r4.s);
}

// An array literal, and a `let` whose `if` builds the tuple.
fn let_array(c: bool) {
    let q5 = mk();
    let [x5, _] = [mk(), q5];
    println!("{}", x5.s);
    let q6 = mk();
    let (a6, _) = if c { (1, q6) } else { (2, mk()) };
    println!("{}", a6);
}

// A destructuring assignment's tuple.
fn assign_tuple() {
    let q7 = mk();
    let a7;
    (a7, _) = (1, q7);
    println!("{}", a7);
}

// `*slot = s`: s.name is b.name now, dropped with b.
fn deref_write() {
    let mut b = B { name: String::new() };
    let slot = &mut b;
    let s = B { name: String::from("s") };
    *slot = s;
    println!("{}", b.name);
}

// The same through a parameter, a field of what a reference points
// to, a `&mut &mut`, and inside an `if`.
fn set(slot: &mut B, s2: B) {
    *slot = s2;
}

fn set_field(o: &mut O, s3: B) {
    o.inner = s3;
}

fn set_twice(pp: &mut &mut B, s4: B) {
    **pp = s4;
}

fn set_if(slot: &mut B, c: bool) {
    let s5 = B { name: String::from("s5") };
    if c {
        *slot = s5;
    }
}

fn main() {
    let_tuple();
    let_wild();
    let_ref();
    let_array(true);
    assign_tuple();
    deref_write();
    let mut b = B { name: String::new() };
    set(&mut b, B { name: String::new() });
    let mut o = O { inner: B { name: String::new() }, n: 1 };
    set_field(&mut o, B { name: String::new() });
    println!("{}", o.n);
    let mut r = &mut b;
    set_twice(&mut r, B { name: String::new() });
    set_if(&mut b, true);
}
