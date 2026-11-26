fn main() {
    let mut n = 0;
    let mut r = &mut n;
    match &mut r {
        s => **s += 1,
    }
    *r += 1;
}
