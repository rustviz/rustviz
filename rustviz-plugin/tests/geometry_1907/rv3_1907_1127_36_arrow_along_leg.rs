fn take_gen<T>(_t: T) {}

fn main() {
    let s = String::from("abc");
    let s2 = String::from("abc");
    let mut vr = vec![s.as_str(), "abc"];
    if vr.is_empty() {
    } else if vr.is_empty() {
        vr.push(s2.as_str());
        if vr.is_empty() {
            take_gen(vr);
        }
    } else {
        let r = &s;
    }
}
