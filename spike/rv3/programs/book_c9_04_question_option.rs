fn first_plus_one(v: &Vec<i32>) -> Option<i32> {
    let x = v.first()?;
    Some(*x + 1)
}

fn main() {
    let v = vec![1, 2];
    println!("{:?}", first_plus_one(&v));
}
