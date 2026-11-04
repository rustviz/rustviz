fn consume(s: String) -> usize {
    s.len()
}

fn main() {
    let n = 2;
    let s = match n % 2 {
        0 => String::from("even"),
        1 => String::from("odd"),
        _ => unreachable!(),
    };
    let t = String::from("t");
    let m = match n {
        0 => consume(t),
        _ => panic!("no"),
    };
    println!("{} {}", s, m);
}
