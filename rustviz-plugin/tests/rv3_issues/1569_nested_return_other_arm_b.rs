fn main() {
    let mut s = String::from("abc");
    let b = true;
    if b {
        let m = &mut s;
        if b {
            if s.len() > 1 {
                return;
            }
        } else {
            m.push('a');
        }
    }
}
