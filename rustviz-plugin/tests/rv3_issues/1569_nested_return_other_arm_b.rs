fn main() {
    let mut s = String::from("abc");
    let (b, c) = (true, std::env::args().len() > 1);
    if b {
        let m = &mut s;
        if c {
            if s.len() > 1 {
                return;
            }
        } else {
            m.push('a');
        }
    }
}
