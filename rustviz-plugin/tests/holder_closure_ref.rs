fn main() {
    let v = vec![1, 2, 3];
    let extra = String::from("x");
    let it = v.iter().map(|x| x + extra.len() as i32);
    let total: i32 = it.sum();
    println!("{} {}", total, extra);
}
