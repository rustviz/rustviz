fn main() {
    let v = vec![String::from("a")];
    let firsts: Vec<String> = v.into_iter().collect();
    let s1 = String::from("x");
    let s2 = String::from("y");
    let s3 = s1 + &s2;
    println!("{:?} {} {}", firsts, s2, s3);
}
