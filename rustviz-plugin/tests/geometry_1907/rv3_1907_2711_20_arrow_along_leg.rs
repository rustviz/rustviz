fn make() -> String {
    String::from("made")
}

fn run(mut s: String, mut o: Option<String>) -> Option<usize> {
    let mut s2 = String::from("hi");
    let s3 = String::from("x");
    let mut v = vec![s3, s];
    if let Some(x) = &mut o {
        match o {
            Some(x2) => {
                let m2 = &mut s2;
            }
            None => {
                v.push(make());
            }
        }
    }
    let s4;
    s4 = String::from("hi");
    Some(0)
}
