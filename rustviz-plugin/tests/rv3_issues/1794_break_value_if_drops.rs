fn g(c: bool) -> usize {
    let n = 'outer: loop {
        let a = String::from("a");
        let b = String::from("b");
        if c { break 'outer if c { a } else { b }; }
    };
    n.len()
}
