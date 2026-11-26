fn main() {
    let b = Box::new(String::new());
    let r2 = &*b;
    'outer: for i in 0..2 {
        for j in 0..2 {
            let r3 = &*b;
            if r2.is_empty() { break 'outer; }
        }
    }
    println!("{}", b);
    println!("{}", r2);
}
