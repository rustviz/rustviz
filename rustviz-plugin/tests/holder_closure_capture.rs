fn main() {
    let s = String::from("hello");
    let k = 2;
    let c = s.chars();
    let lens = s.split('l').map(|p| p.len() * k);
    let n = c.count() + lens.sum::<usize>();
    println!("{} {}", n, s);
}
