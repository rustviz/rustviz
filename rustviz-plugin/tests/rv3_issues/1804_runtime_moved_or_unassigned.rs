pub fn f(c: bool) {
    let x: String;
    if c {
        x = String::new();
        if let _ = () {
            drop(x);
        }
    }
    let _k = 0;
}
