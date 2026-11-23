// #1330: `[a, b]` with a, b `&mut String`. An array literal's elements
// are coercion sites for the type of the first one: a has no expected
// type yet and is moved into k, but b is coerced to a's type with an
// implicit reborrow, `&mut *b`. So b isn't moved: k holds a borrow
// through b, b can't be used while k is live, and b can be used again
// after k's last use (`b.push('?')` compiles there). A tuple's fields
// get no such coercion: `(a, b)` moves both.

// `&mut` parameters.
fn params(a: &mut String, b: &mut String) {
    let k = [a, b];
    k[1].push('!');
    b.push('?');
}

// Local `&mut`s.
fn locals() {
    let mut s = String::new();
    let mut t = String::new();
    let a = &mut s;
    let b = &mut t;
    let k = [a, b];
    k[1].push('!');
    b.push('?');
    println!("{} {}", s, t);
}

// Three elements: b and c are reborrowed; b's borrow ends with k's.
fn three(a: &mut String, b: &mut String, c: &mut String) {
    let k = [a, b, c];
    k[2].push('!');
    c.push('?');
}

// `vec![a, b]` builds the same array.
fn in_vec() {
    let mut s = String::new();
    let mut t = String::new();
    let a = &mut s;
    let b = &mut t;
    let k = vec![a, b];
    drop(k);
    b.push('?');
    println!("{} {}", s, t);
}

// The reborrows written out: b's borrow ends where k's does.
fn written(a: &mut String, b: &mut String) {
    let k = [&mut *a, &mut *b];
    k[1].push('!');
}

// Coerced to `[&String; 2]`: both are immutable reborrows.
fn shared(a: &mut String, b: &mut String) {
    let k: [&String; 2] = [a, b];
    println!("{}", k[1]);
    a.push('?');
}

// A tuple moves both.
fn tuple(a: &mut String, b: &mut String) {
    let k = (a, b);
    k.1.push('!');
}

fn main() {
    let mut x = String::new();
    let mut y = String::new();
    let mut z = String::new();
    params(&mut x, &mut y);
    locals();
    three(&mut x, &mut y, &mut z);
    in_vec();
    written(&mut x, &mut y);
    shared(&mut x, &mut y);
    tuple(&mut x, &mut y);
}
