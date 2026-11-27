// Unwinding, another iteration, and an empty iterator have distinct exits.
fn scan(xs: &[u32]) {
    for x in xs {
        if *x == 0 {
            panic!("{}", x);
        } else {
            continue;
        }
    }
}

fn main() {
    scan(&[]);
    scan(&[1, 2]);
}
