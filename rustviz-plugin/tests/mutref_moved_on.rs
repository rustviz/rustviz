// Moving a `&mut` hands its borrow on: the moved-from variable holds
// nothing afterwards, so only the last holder ends the borrow and gives
// it back to the lender (#209).

fn main() {
    let mut s = String::from("hi");
    let r = &mut s;
    let r2 = r;          // r is moved; using r after this is E0382
    let r3 = r2;         // and so is r2
    r3.push_str("!");    // r3's last use: the borrow of s ends here

    let mut t = String::from("t");
    let mut q = &mut t;
    let q2 = q;          // q is moved ...
    q2.push('.');
    q = &mut s;          // ... so this assignment ends no borrow of t
    q.push(',');
    println!("{} {}", s, t);
}
