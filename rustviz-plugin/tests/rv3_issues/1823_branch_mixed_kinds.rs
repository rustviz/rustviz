fn f(flag: bool) {
    let mut d = String::from("d");
    let e = String::from("e");
    let rd: &String = &mut d;
    let o = Some(if flag { rd } else { &e });
    println!("{:?}", o);
}
