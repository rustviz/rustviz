fn id<T>(t: T) -> T {
    t
}

fn run(o: Option<String>) -> Option<usize> {
    let s2 = String::from("hi");
    if let Some(x) = &o && x.len() > 1 {
        let mut s4 = if s2.len() > 1 {
            let mut s3 = id(s2);
            std::mem::replace(&mut s3, String::new())
        } else {
            String::from("hey")
        };
    }
    Some(0)
}
