fn take(_s: String) {}

fn main() {
    let s = String::from("hey");
    let mut s2 = format!("{}!", s);
    if s.len() > 1 {
        let r = &s;
    } else if s2.len() > 1 {
        let s3;
        if s2.len() > 1 { } else { s3 = std::mem::take(&mut s2); }
    } else {
        if s2.len() > 1 {
            return;
        }
    }
}
