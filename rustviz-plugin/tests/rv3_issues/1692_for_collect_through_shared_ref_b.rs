fn main() {
    let mut w = vec![&1];
    let v = &w;
    for x in v
        .iter()
        .copied()
        .collect::<Vec<_>>()
    {
        w.clear();
        println!("{x}");
    }
}
