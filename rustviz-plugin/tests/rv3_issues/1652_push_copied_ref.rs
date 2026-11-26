fn pushed() {
    let mut s = String::from("hi");
    let mut vr: Vec<&String> = vec![];
    let r: &String = &mut s;
    vr.push(r);
    println!("{:?}", vr);
}
