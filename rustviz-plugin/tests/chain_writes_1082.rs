fn ch(a: &mut Vec<i32>, b: &mut Vec<i32>) {
    for x in a.iter_mut().chain(b.iter_mut()) {
        *x += 1;
    }
}
fn z(c: &mut Vec<i32>, d: &mut Vec<i32>) {
    for (x, y) in std::iter::zip(c.iter_mut(), d.iter_mut()) {
        *y += *x;
    }
}
fn moved(e: &mut Vec<String>, f: &mut Vec<String>) {
    for x in e.iter_mut().chain(f) {
        *x = String::new();
    }
}
fn main() {
    let mut p = vec![1];
    let mut q = [2, 3];
    for x in p.iter_mut().chain(q.iter_mut()).rev() {
        *x *= 2;
    }
    ch(&mut p, &mut vec![2]);
    z(&mut p, &mut vec![2]);
    moved(&mut vec![String::new()], &mut vec![String::new()]);
}
