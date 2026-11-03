// The four ways to take a reference from a reference. Only the
// `&*r` / `&mut *r` forms are reborrows (borrowing *through* r);
// `&r` / `&mut r` are fresh borrows of the variable r itself, so
// ra2 / rc2 are `&String`-shaped while rb2 / rd2 are references to
// references.

fn main() {
    let mut a = String::from("a");
    let ra = &mut a;
    let ra2 = &mut *ra;     // mutable reborrow: ra unusable while ra2 is live
    ra2.push_str("!");
    ra.push_str("?");

    let mut b = String::from("b");
    let mut rb = &mut b;
    let rb2 = &mut rb;      // fresh mutable borrow of rb: &mut &mut String
    rb2.push_str("!");

    let c = String::from("c");
    let rc = &c;
    let rc2 = &*rc;         // immutable reborrow
    let n = rc2.len();

    let d = String::from("d");
    let rd = &d;
    let rd2 = &rd;          // fresh immutable borrow of rd: &&String
    let m = rd2.len();
}
