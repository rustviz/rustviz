fn main() {
    let b = true;
    let mut v = vec![1];
    if b {
        while let Some(x) = v.pop() {
            println!("{}", x);
        }
    } else {
        v.push(2);
    }
}
