// A borrow can outlive the variable it was first stored in: moving a
// `&mut` (or copying a `&`) hands the same loan on, so the lender
// stays unusable / read-only until the *new* holder's last use.

fn main() {
    let mut s = String::from("s");
    let y = &mut s;
    let z = y;              // the &mut moves to z; s stays unusable
    z.push_str("!");

    let t = String::from("t");
    let a = &t;
    let b = a;              // the & is copied to b; t stays read-only
    let n = b.len();
}
