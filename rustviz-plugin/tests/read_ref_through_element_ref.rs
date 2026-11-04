// #453: reading a *reference* out through a reference (or Box) stored
// in a collection. `let r = *rr[0];` with rr: `Vec<&&i32>` copies the
// `&i32` that the element points to; rr is a Vec, not a borrower, so
// there was no lender to look up and the plugin panicked. Which
// variable the element points to isn't known statically, so r is drawn
// as a reference copied out of rr's column, with no lender named.
fn main() {
    let a = 1;
    let ra = &a;
    let rr = vec![&ra];
    let r = *rr[0];
    let rrr = vec![&rr];
    let q = *rrr[0][0];
    let b = Box::new(&a);
    let s = *b;
    println!("{} {} {}", r, q, s);
}
