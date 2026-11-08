fn main() {
    let pairs = vec![(1, String::from("a")), (2, String::from("b"))];
    for (n, s) in &pairs {
        println!("{} {}", n, s);
    }
    let mut m = vec![(1, String::from("c"))];
    for (n, s) in &mut m {
        s.push('x');
        println!("{}", n);
    }
    println!("{} {}", pairs.len(), m.len());
}
