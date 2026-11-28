fn main() {
    let o = Some(String::from("hey"));
    if let Some(x2) = &o && x2.len() > 1 {
        let s4 = match o {
            Some(y) => y,
            None => String::new(),
        };
    }
}
