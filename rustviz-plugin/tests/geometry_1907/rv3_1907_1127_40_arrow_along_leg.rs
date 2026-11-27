fn downgrade(s: &mut String) -> &String {
    s
}

fn main() {
    let mut s4 = String::from("ab");
    let x5 = &s4[..];
    let b = Box::new(String::from("x"));
    if x5.len() > 1 {
        let x6 = &s4[..];
        let r2 = downgrade(&mut s4);
    } else {
        if x5.len() > 1 {
            return;
        }
    }
}
