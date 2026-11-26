pub fn f(t: (bool, String, String)) {
    match t {
        (true, x, y) | (false, y, x) => println!("{x}{y}"),
    }
}
