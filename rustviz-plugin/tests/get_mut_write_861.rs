use std::collections::HashMap;

struct Bag {
    items: Vec<String>,
}

fn main() {
    let mut v = vec![1, 2, 3];
    *v.get_mut(0).unwrap() += 1;
    if let Some(x) = v.get_mut(1) {
        *x += 10;
    }
    let y = v.get_mut(2).unwrap();
    *y = 7;
    for e in v.iter_mut() {
        *e *= 2;
    }
    let mut m: HashMap<&str, i32> = HashMap::new();
    m.insert("k", 1);
    *m.get_mut("k").unwrap() = 5;
    let mut names = vec![String::from("a"), String::from("b")];
    *names.get_mut(0).unwrap() = String::from("c");
    let n = &mut names[1];
    *n = String::from("d");
    let mut b = Bag { items: vec![String::from("e")] };
    let it = b.items.get_mut(0).unwrap();
    *it = String::from("f");
    println!("{:?} {:?} {:?} {:?}", v, m, names, b.items);
}
