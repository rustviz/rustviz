fn run(s: String, mut o: Option<String>, flag: bool) -> Option<usize> {
    let mut s2 = s;
    if let Some(x2) = o {
        if flag {
            s2 = String::from("hey");
            o = Some(String::from("hi"));
        }
    }
    Some(0)
}
