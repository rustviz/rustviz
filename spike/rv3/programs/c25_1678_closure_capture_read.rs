fn main() {
    let list = vec![1, 2, 3];
    let only_borrows = || println!("{list:?}");
    println!("{list:?}");
    only_borrows();
}
