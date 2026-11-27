fn downgrade(s: &mut String) -> &String {
    s
}

fn main() {
    let mut s = String::from("ab");
    if s.len() > 1 {
        let mut r = &s;
        r = downgrade(&mut s);
    } else {
    }
}
