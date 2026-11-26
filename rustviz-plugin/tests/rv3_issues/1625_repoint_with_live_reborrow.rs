fn main() {
    let mut x = 0;
    let mut y = 0;
    let mut r = &mut x;
    let a = &mut *r;
    r = &mut y;
    *r = 1;
    *a = 2;
}
