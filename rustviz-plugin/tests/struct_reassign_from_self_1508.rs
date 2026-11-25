// #1508, #1509: a struct (or tuple) assigned a new value built from its
// own old value. The right-hand side runs first: it moves the old fields
// out, then the assignment stores the new value, so every field holds a
// value again afterwards and is dropped at the scope end (no drop flag).
struct Pair {
    a: String,
    b: String,
}

impl Pair {
    fn into_a(self) -> String {
        self.a
    }
}

fn take(p: Pair) -> String {
    p.b
}

fn first(t: (String, String)) -> String {
    t.0
}

// #1508: `p.a` moved into the new value inside a branch comes straight back.
fn field_back_in_branch() {
    let mut p = Pair { a: String::from("a"), b: String::from("b") };
    if p.b.len() > 1 {
        p = Pair { a: p.a, b: String::from("c") };
    }
    println!("{}", p.a);
}

fn field_back_used_in_arm(c: bool) {
    let mut w = Pair { a: String::from("a"), b: String::from("b") };
    if c {
        w = Pair { a: w.a, b: String::from("c") };
        println!("{}", w.a);
    }
    println!("{}", w.b);
}

fn field_back_in_both_arms(c: bool) {
    let mut q = Pair { a: String::from("a"), b: String::from("b") };
    if c {
        q = Pair { a: q.a, b: String::from("c") };
    } else {
        q = Pair { b: q.b, a: String::from("d") };
    }
    println!("{} {}", q.a, q.b);
}

fn field_back_in_match(n: i32) {
    let mut m = Pair { a: String::from("a"), b: String::from("b") };
    match n {
        0 => m = Pair { a: m.a, b: String::from("c") },
        _ => {}
    }
    println!("{}", m.a);
}

fn tuple_part_back_in_branch(c: bool) {
    let mut t = (String::from("a"), String::from("b"));
    if c {
        t = (t.0, String::from("c"));
    }
    println!("{}", t.0);
}

// #1509: the right-hand side moves all of the old value (`p.into_a()`),
// then the new value's fields arrive.
fn whole_moved_by_method() {
    let mut x = Pair { a: String::from("a"), b: String::from("b") };
    let s2 = String::from("c");
    x = Pair { a: s2, b: x.into_a() };
    println!("{}", x.a);
}

fn whole_moved_fields_swapped() {
    let mut y = Pair { a: String::from("a"), b: String::from("b") };
    let s3 = String::from("c");
    y = Pair { b: y.into_a(), a: s3 };
    println!("{}", y.a);
}

fn whole_moved_by_fn() {
    let s4 = String::from("x");
    let mut z = Pair { a: String::from("a"), b: String::from("b") };
    z = Pair { a: s4, b: take(z) };
    println!("{}", z.a);
}

fn whole_tuple_moved() {
    let s5 = String::from("x");
    let mut u = (String::from("a"), String::from("b"));
    u = (s5, first(u));
    println!("{}", u.0);
}

fn main() {
    field_back_in_branch();
    field_back_used_in_arm(true);
    field_back_in_both_arms(true);
    field_back_in_match(0);
    tuple_part_back_in_branch(true);
    whole_moved_by_method();
    whole_moved_fields_swapped();
    whole_moved_by_fn();
    whole_tuple_moved();
}
