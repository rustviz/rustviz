fn main() {
    let a = 1;
    let mut b = 2;
    let r = &mut b;
    let h: Option<(&i32, &i32)> = Some((&a, r));
    let n = *r + h.unwrap().1;
    *r = 3;
}
