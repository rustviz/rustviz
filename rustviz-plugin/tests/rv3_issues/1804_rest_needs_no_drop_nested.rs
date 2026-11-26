pub struct NC;
pub fn f() {
    let t = ((String::new(), true), true);
    let _s = t.0.0;
    let _k = 0;
}
pub fn g(c: bool) {
    let t = (NC, String::new());
    let _s = t.1;
    if c {
        let _n = t.0;
    }
    let _k = 0;
}
