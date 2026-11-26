fn copied_both_ways(c: bool) {
    let s5 = String::from("a");
    let t5 = String::from("b");
    let mut a5 = &s5;
    let mut b5 = a5;
    if c {
        b5 = a5; a5 = b5; a5.len(); b5.len();
    } else {
        println!("{}", t5);
    }
    println!("{}", t5);
}
