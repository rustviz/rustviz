use std::collections::HashMap;

fn main() {
    let v = vec![10, 20, 30];
    let f = v.first().unwrap();
    let s = String::from("hi");
    let t = s.as_str();
    let mut scores = HashMap::new();
    scores.insert(String::from("Blue"), 10);
    let team = String::from("Blue");
    let score = scores.get(&team).unwrap();
    println!("{} {} {}", f, t, score);
}
