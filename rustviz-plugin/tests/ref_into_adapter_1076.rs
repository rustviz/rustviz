fn shared(a: &mut Vec<i32>, b: &Vec<i32>) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += *y + b.len() as i32;
    }
}
fn moved(a: &mut Vec<i32>, b: &mut Vec<i32>) {
    for (x, y) in a.iter_mut().zip(b) {
        *y += *x;
    }
}
fn chained(a: &mut Vec<i32>, b: &Vec<i32>) {
    for x in a.iter().chain(b) {
        println!("{}", x);
    }
}
fn main() {
    let mut p = vec![1];
    let mut q = vec![2];
    shared(&mut p, &q);
    moved(&mut p, &mut q);
    chained(&mut p, &q);
    let r = &mut q;
    for (x, y) in p.iter().zip(r) {
        *y += *x;
    }
    println!("{}", q.len());
}
