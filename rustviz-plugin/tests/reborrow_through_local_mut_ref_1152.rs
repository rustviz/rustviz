fn iter_holder() {
    let mut v = vec![1, 2, 3];
    let r1 = &mut v;
    let mut it = r1.iter_mut();
    let _x = it.next();
    r1.push(4);
}
fn first_ref() {
    let mut v = vec![1, 2, 3];
    let r2 = &mut v;
    let x = r2.first_mut().unwrap();
    *x += 1;
    r2.push(4);
}
fn get_opt() {
    let mut v = vec![1, 2, 3];
    let r3 = &mut v;
    let o = r3.get_mut(0);
    if let Some(e) = o {
        *e += 1;
    }
    r3.push(4);
}
fn in_loop() {
    let mut v = vec![1, 2, 3];
    let r4 = &mut v;
    for _ in 0..2 {
        let y = r4.last_mut().unwrap();
        *y += 1;
    }
    r4.push(4);
}
fn main() { iter_holder(); first_ref(); get_opt(); in_loop(); }
