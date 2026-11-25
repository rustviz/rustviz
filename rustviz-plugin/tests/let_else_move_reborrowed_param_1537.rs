fn run(m: &mut String, mut o: Option<String>) -> Option<usize> {
    let r2 = &*m;
    let Some(x) = o else {
        let m2 = m;
        return None;
    };
    Some(r2.len())
}
