fn main() {
    let s = String::from("x");
    let vr = vec![s.as_str(), "abc"];
    if vr.is_empty() {
        let r = &s;
    } else if vr.is_empty() {
    } else {
        println!("{:?}", vr);
    }
}
