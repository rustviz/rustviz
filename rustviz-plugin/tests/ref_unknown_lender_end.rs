// #463: a reference whose lender isn't known statically. Its borrow
// ends, but there is no column to return it to: the end must not read
// "Return immutably borrowed resource from r to *r" (r returned to
// itself).
//
// `pick()`'s result borrows 'static data that the caller can't name;
// `*rr[0]` copies a reference out of a collection (which variable it
// points to isn't known), and so does `*b` out of a `Box<&i32>`.
fn pick() -> &'static i32 {
    &5
}

fn from_vec(rr: Vec<&&i32>) {
    let r = *rr[0];
    println!("{}", r);
}

fn from_box(b: Box<&i32>) {
    let s = *b;
    println!("{}", s);
}

fn main() {
    let r = pick();
    println!("{}", r);
    let x = 1;
    let rx = &x;
    from_vec(Vec::new());
    from_box(Box::new(rx));
}
