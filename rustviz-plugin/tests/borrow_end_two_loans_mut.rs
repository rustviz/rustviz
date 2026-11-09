// The mutable variant of #803: the `then` arm's dot ends c's loan of `a` and
// its unused new loan of `b`, and names each lender.
fn main() {
    let mut a = 1;
    let mut b = 2;
    let mut c = &mut a;
    let flag = true;
    if flag {
        c = &mut b;
    } else {
        *c += 1;
    }
}
