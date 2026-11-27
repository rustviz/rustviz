fn run(mut o: Option<String>) -> Option<usize> {
    let mut s = String::from("hey");
    let o2 = Some(s.to_uppercase());
    if let Some(x) = o2 {
        let m2 = &mut s;
        let r = o.as_ref().unwrap();
        match o {
            Some(x2) if x2.len() > 1 => {
                let mut c = || s.push('!');
            }
            Some(_) => {}
            None => {}
        }
    }
    Some(0)
}
