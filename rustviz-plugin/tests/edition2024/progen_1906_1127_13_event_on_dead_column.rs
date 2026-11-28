fn run(mut o: Option<String>) -> Option<usize> {
    match o {
        Some(ref x) if x.len() > 1 => {
            match o.as_ref() {
                Some(x2) => {}
                None => {
                    let r2 = x;
                }
            }
        }
        Some(_) => {
        }
        None => {
        }
    }
    Some(0)
}
