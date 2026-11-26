fn main() {
    let v = &mut vec![&1];
    for x in v.iter().copied().collect::<Vec<_>>() {
        v.clear();
        println!("{x}");
    }
}
