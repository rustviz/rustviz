fn repointed(c: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let mut p = &s;
    if c {
        println!("{}", p);
        p = &t;
        p.len();
    } else {
        println!("{}", t);
    }
    println!("{}", s);
}
