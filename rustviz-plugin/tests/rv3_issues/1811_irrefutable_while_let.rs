#![allow(irrefutable_let_patterns)]
fn main() {
    let mut n = 0;
    let mut r = &mut n;
    while let s = &mut r {
        **s += 1;
        break;
    }
    assert_eq!(n, 1);
}
