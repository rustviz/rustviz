fn f2(c: bool) {
    let mut b: Box<Box<String>> = Box::new(Box::new(String::from("a")));
    if c {
        let _x = *b;
        return;
    }
    **b = String::new();
}
