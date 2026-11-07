fn main() {
    let s = String::from("hello");
    let k = 2;
    let lens = s.split('l').map(|p| p.len() * k);
    println!("{:?}", lens.collect::<Vec<_>>());
}
