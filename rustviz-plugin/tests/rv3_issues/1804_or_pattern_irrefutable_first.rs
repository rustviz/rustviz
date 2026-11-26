pub fn g(t: (String, String)) {
    match t {
        (x, _) | (_, x) => println!("{x}"),
    }
}
