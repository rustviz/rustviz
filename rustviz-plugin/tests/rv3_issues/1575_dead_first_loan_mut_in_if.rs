fn main() {
    let mut s = String::from("hi");
    let mut r2 = &s;
    let b = true;
    if b {
        let m3 = &mut s;
        m3.push('a');
    }
    let t = String::new();
    r2 = &t;
}
