fn put<'a>(left: &mut &'a String, right: &mut &'a String, v: &'a String) { *right = v; let _ = left; }
fn main() {
    let a = String::new(); let t = String::new();
    let mut left = vec![&a]; let mut right = vec![&a];
    put(&mut left[0], &mut right[0], &t);
    println!("{}", left[0]);
    println!("{t}");
    println!("{}", right[0]);
}
