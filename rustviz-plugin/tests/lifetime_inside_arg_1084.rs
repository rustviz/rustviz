// #1084: a returned reference whose lifetime appears only inside an
// argument's type (`&Vec<&'a Vec<i32>>`) borrows what the references
// stored in that argument point to, not the argument itself: the caller
// can still change `rows` while `f` is in use.
fn inner<'a>(rows: &Vec<&'a Vec<i32>>) -> &'a i32 {
    &rows[0][1]
}

fn copied_out<'a>(names: &Vec<&'a str>) -> &'a str {
    names.first().copied().unwrap()
}

// The lifetime is the argument's own: the result borrows `rows` too.
fn outer<'a>(rows: &'a Vec<&'a Vec<i32>>) -> &'a i32 {
    &rows[0][0]
}

// One argument tied directly, the other through what it stores.
fn pick<'a>(x: &'a str, names: &Vec<&'a str>) -> &'a str {
    if names.is_empty() { x } else { names[0] }
}

// Two arguments, each tied only through what it stores.
fn both<'a>(a: &Vec<&'a str>, b: &Vec<&'a str>) -> &'a str {
    if a.is_empty() { b[0] } else { a[0] }
}

// What `x` points to is the reference the result copies (`T = &String`).
fn get<T: Copy>(x: &T) -> T {
    *x
}

fn main() {
    let row = vec![5, 6];
    let mut rows = vec![&row];
    let f = inner(&rows);
    rows.push(&row);
    println!("{}", f);
    let s = String::from("hi");
    let names = vec![s.as_str()];
    let g = copied_out(&names);
    println!("{}", g);
    let col = vec![7, 8];
    let cols = vec![&col];
    let o = outer(&cols);
    println!("{} {}", o, cols.len());
    let t = String::from("yo");
    let u = String::from("hey");
    let others = vec![u.as_str()];
    let p = pick(&t, &others);
    println!("{}", p);
    let a1 = String::from("a");
    let b1 = String::from("b");
    let mut va = vec![a1.as_str()];
    let vb = vec![b1.as_str()];
    let z = both(&va, &vb);
    va.push("c");
    println!("{} {}", z, va.len());
    let w = String::from("w");
    let rw = &w;
    let c = get(&rw);
    println!("{}", c);
}
