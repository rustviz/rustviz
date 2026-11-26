fn main() {
    let mut n = 0;
    let mut r = &mut n;
    let s = &mut r;
    **s += 1;
    let shared = &r;
    let v = **shared;
    assert_eq!(v, 1);
}
