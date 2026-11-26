pub fn f(c: bool, d: bool) -> i32 {
    let t = (true, String::new(), String::new());
    let _a = t.1;
    let _b = t.2;
    if c {
        return 1;
    }
    if d {
        return 2;
    }
    0
}
