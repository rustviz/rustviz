fn main() {
    let v = vec![String::new()];
    let s = String::new();
    if let Some(x) = v.last() {
        let r = x;
    } else {
        let mut n = 0;
        while n < 3 {
            n += 1;
        }
        let b = Box::new(s);
    }
}
