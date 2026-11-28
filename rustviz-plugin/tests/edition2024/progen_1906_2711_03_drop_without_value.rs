fn run(m: &mut String) -> Option<usize> {
    let s2 = m.to_string();
    let v = vec![String::from("ab"), s2];
    if let [x, rest @ ..] = v.as_slice() {
        let s5 = 'blk: {
            String::from("rust")
        };
    }
    Some(0)
}
