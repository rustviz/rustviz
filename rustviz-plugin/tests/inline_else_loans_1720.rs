fn main() {
    let mut s2 = String::from("rust");
    let m = &mut s2;
    if m.len() > 2 {
        let n = m.len();
        let r3 = &mut s2;
        let m2 = &mut s2;
    } else { m.push('a'); }
}
