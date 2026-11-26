// Known bug, from the closed 2.x fix PRs (fix/loans-1632-1635); kept as a RustViz 3 regression case.
// Issue(s): #1632 (see known_bugs/README.md for what main draws wrong).
// #1632: a call that stores a borrow of one field into its receiver
// through a trait bound (`extend<I: IntoIterator<Item = T>>`) borrows
// that field, not the whole value: the other fields stay usable.
// (`v.push(&p.0)` already said so.)
struct Q { a: i32, b: i32 }

fn array_of_field() {
    let mut p = (1, 2);
    let mut v: Vec<&i32> = Vec::new();
    v.extend([&p.0]);
    p.1 = 3;
    let n = *v[0];
    println!("{n} {}", p.1);
}

fn vec_of_named_field() {
    let mut q = Q { a: 1, b: 2 };
    let mut w: Vec<&i32> = Vec::new();
    w.extend(vec![&q.a]);
    q.b = 3;
    let m = *w[0];
    println!("{m} {}", q.b);
}

fn mut_field() {
    let mut t = (1, 2);
    let mut u: Vec<&mut i32> = Vec::new();
    u.extend([&mut t.0]);
    t.1 = 3;
    *u[0] = 4;
    println!("{}", t.1);
}

fn nested_field() {
    let mut d = ((1, 2), 3);
    let mut x: Vec<&i32> = Vec::new();
    x.extend(Some(&(d.0).1));
    d.1 = 4;
    let k = *x[0];
    println!("{k} {}", d.1);
}

// Fields of one variable lent both ways in one store: one kind is drawn
// for every lender of a store, so these keep the whole-variable drawing
// rather than a field drawn with the other field's kind.
fn mixed_kinds() {
    let mut e = (1, 2);
    let mut y: Vec<(&i32, &mut i32)> = Vec::new();
    y.extend([(&e.0, &mut e.1)]);
    *y[0].1 = 5;
}

// A field inside an element (`p2.0[0].0`): no column of its own, drawn as
// p2.
fn field_of_element() {
    let mut p2 = ([(1, 2)], 3);
    let mut z: Vec<&mut i32> = Vec::new();
    z.extend([&mut p2.0[0].0]);
    *z[0] = 5;
}

fn main() {
    mixed_kinds();
    field_of_element();
    array_of_field();
    vec_of_named_field();
    mut_field();
    nested_field();
}
