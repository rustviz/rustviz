fn arms(flag: bool) {
    let mut s = String::from("s");
    let r: &String = &mut s;
    let v = if flag { Some(r) } else { Some(&s) };
    println!("{:?}", v);
}
