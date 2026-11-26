struct P<'a> { n: &'a mut i32 }
fn main() {
    let mut x = 0;
    let mut p = P { n: &mut x };
    let r = &mut p;
    *r.n = 1;
    *p.n = 2;
}
