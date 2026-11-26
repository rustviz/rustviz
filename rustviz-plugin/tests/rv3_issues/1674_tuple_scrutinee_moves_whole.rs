fn f(z: Option<String>) {
    if let (Some(c), 0) = (z, 1) {
        println!("{}", c);
    }
}
