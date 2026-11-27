fn main() {
    let s = String::from("rust");
    let mut s = s;
    let mut n = 0;
    while n < 3 {
        n += 1;
        let m2 = &mut s;
        let r = &s;
        if n > 0 { continue; }
    }
}
