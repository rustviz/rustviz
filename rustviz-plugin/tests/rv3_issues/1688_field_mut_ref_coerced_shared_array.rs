struct S<'a> { p: &'a mut i32 }
fn main() {
    let a = 1;
    let mut b = 2;
    let s = S { p: &mut b };
    let h: [&i32; 2] = [&a, s.p];
    let n = *s.p + *h[1];
    *s.p = 3;
}
