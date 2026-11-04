fn main() {
    let v = vec![1, 2, 3];
    let head: Option<&i32> = v.first();
    if let Some(x) = head {
        println!("{}", x);
    }
    if let Some(y) = v.get(1) {
        println!("{}", y);
    }
}
