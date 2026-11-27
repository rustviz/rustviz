fn run(mut o: Option<String>) -> Option<usize> {
    let s = String::from("x");
    let s2 = s;
    if let Some(x3) = &o {
        if s2.len() > 1 {
            let s4 = o?;
        }
    }
    Some(0)
}
