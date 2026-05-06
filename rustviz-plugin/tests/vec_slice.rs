// Slicing a `Vec` (`&v[..]`) — the macro-expanded `vec![..]` RHS
// used to crash match_rhs (it walked the desugared `<[_]>::into_vec(...)`
// Call without the function being registered as a RAP), and the
// `&v[..]` borrow used to fall back to an Anonymous lender. Now `v`
// is a single owner column (Vec collapsed via `ty_is_special_owner`)
// and the borrow is attributed to it.

fn main() {
    let v = vec![1, 2, 3];
    let p = &v[..];
    println!("{:?}", p);
}
