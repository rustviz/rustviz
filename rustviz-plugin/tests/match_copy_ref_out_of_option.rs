fn main() {
    let v = vec![1, 2, 3, 4, 5];
    let third: Option<&i32> = v.get(2);
    match third {
        Some(x) => println!("The third element is {x}"),
        None => println!("There is no third element."),
    }
}
