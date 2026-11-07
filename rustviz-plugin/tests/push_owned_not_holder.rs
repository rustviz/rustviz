fn main() {
    let s = String::from("s");
    let mut names: Vec<String> = Vec::new();
    names.push(s.clone());
    let k = 2;
    let mut nums = vec![1, 2, 3];
    nums.retain(|n| *n > k);
    nums.push(4);
    println!("{:?} {:?} {}", names, nums, s);
}
