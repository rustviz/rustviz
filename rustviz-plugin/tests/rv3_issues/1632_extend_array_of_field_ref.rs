fn main() {
    let mut p = (1, 2);
    let mut v: Vec<&i32> = Vec::new();
    v.extend([&p.0]);
    p.1 = 3;
    let n = *v[0];
}
