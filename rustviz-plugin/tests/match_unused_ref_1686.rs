fn main() {
    let b = Box::new(String::new());
    let x = b.as_str();
    match b.as_str() {
        "" => {}
        _ => {
            if x.len() > 1 {
                let r = &*b;
                println!("{}", b);
            } else if x.len() > 1 {
                println!("{}", b);
            }
        }
    }
}
