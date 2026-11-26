fn main() {
    let mut v = vec![1, 2];
    for (x, y) in v.iter_mut().map(|p| *p).collect::<Vec<_>>().into_iter().zip(v.iter()) {
        let n = v.len();
        println!("{x} {y} {n}");
    }
}
