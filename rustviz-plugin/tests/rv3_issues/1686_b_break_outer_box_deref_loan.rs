fn main() {
    let mut b = Box::new(String::from("a"));
    let r2 = &*b;
    'outer: for i in 0..2 {
        for j in 0..2 {
            if r2.is_empty() { break 'outer; }
        }
    }
    println!("{}", b);
    println!("{}", r2);
}
