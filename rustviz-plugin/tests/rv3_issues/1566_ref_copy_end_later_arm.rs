fn run(v: Vec<String>, s: String) {
    let r = &s;
    match &v[..] {
        [] => {}
        [x2] => {
            let h = r;
        }
        [x, rest @ ..] => {}
    }
    println!("{}", r);
}
