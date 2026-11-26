fn main() {
    let mut v = Vec::<usize>::new();
    let r = &mut v;
    r.push(
        r.len()
    );
}
