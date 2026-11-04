fn main() {
    let mut list = [3, 1, 2];
    let mut num_sort_operations = 0;
    list.sort_by_key(|r| {
        num_sort_operations += 1;
        *r
    });
    println!("{:?} {}", list, num_sort_operations);
}
