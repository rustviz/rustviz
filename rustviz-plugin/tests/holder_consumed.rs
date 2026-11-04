fn main() {
    let v = vec![1, 2, 3];
    let it = v.iter();
    let total: i32 = it.sum();
    println!("{} {:?}", total, v);
}
