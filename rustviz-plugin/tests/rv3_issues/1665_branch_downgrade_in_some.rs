fn downgrade(s: &mut String) -> &String { s }
fn made_shared(flag: bool) {
    let mut a9 = String::from("a");
    let e9 = String::from("e");
    let o9 = Some(if flag { downgrade(&mut a9) } else { &e9 });
    println!("{:?}", o9);
}
