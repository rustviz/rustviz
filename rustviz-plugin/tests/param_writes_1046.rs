fn bump_all(v: &mut Vec<i32>) {
    for e in v.iter_mut() {
        *e += 1;
    }
}
fn double_then_push(w: &mut Vec<i32>) {
    for e in w.iter_mut() {
        *e *= 2;
    }
    w.push(0);
}
fn through_local_ref() {
    let mut s = vec![1, 2];
    let r = &mut s;
    for e in r.iter_mut() {
        *e += 1;
    }
    println!("{:?}", s);
}
fn set(t: &mut Vec<i32>) {
    let y = t.get_mut(0).unwrap();
    *y = 7;
}
fn replace(u: &mut Vec<i32>) {
    let q = &mut *u;
    *q = vec![9];
}
fn add_one(n: &mut i32) {
    let x = &mut *n;
    *x += 1;
}
fn main() {
    let mut nums = vec![1, 2];
    bump_all(&mut nums);
    double_then_push(&mut nums);
    through_local_ref();
    set(&mut nums);
    replace(&mut nums);
    let mut k = 1;
    add_one(&mut k);
    println!("{:?} {}", nums, k);
}
