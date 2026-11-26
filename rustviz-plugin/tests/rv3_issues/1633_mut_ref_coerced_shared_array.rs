fn main() {
    let a = 1;
    let mut b = 2;
    let r = &mut b;
    let h: [&i32; 2] = [&a, r];
    let n = *r + *h[1];
    *r = 3;
}
