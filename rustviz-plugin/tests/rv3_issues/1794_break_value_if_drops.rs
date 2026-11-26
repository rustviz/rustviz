fn g(c: bool, d: bool) -> usize {
    let n = 'outer: loop {
        let a = String::from("a");
        let b = String::from("b");
        if c { break 'outer if d { a } else { b }; }
    };
    n.len()
}
