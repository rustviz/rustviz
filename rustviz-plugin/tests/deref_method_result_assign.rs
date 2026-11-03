use std::collections::HashMap;

fn main() {
    let mut v = vec![1, 2, 3];
    *v.last_mut().unwrap() = 5;
    let mut map = HashMap::new();
    let k = String::from("a");
    *map.entry(k).or_insert(0) += 1;
    println!("{:?} {:?}", v, map);
}
