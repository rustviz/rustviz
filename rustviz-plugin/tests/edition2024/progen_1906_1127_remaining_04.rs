fn main() {
    let s = String::from("hey");
    let mut vr = vec![s.as_str(), "ab"];
    for e in vr.iter() {
        if vr.is_empty() {
        } else {
            let mut s2 = e.to_string();
        }
    }
    loop {
        println!("{:?}", vr);
        break;
    }
}
