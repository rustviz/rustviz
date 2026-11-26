fn main() {
    let a = 1;
    let mut b = 2;
    let o = Some(&a).filter(|_| false);
    let r = o.unwrap_or_else(|| &b);
    assert_eq!(*r, 2);
}
