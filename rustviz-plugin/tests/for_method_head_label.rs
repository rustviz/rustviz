fn main() {
    let mut v = vec![1, 2, 3];
    for x in v.iter_mut() {
        *x += 1;
    }
    let w = vec![String::from("a")];
    for s in w.iter() {
        println!("{}", s);
    }
    let t = String::from("hi");
    for c in t.chars() {
        println!("{}", c);
    }
}
