fn bump(v: &mut Vec<i32>) {
    let x = v.get_mut(0).unwrap();
    *x += 1;
}
fn bump_index(w: &mut Vec<i32>) {
    let r = &mut w[1];
    *r *= 2;
}
fn maybe(u: &mut Vec<i32>) {
    if let Some(m) = u.get_mut(1) {
        *m += 10;
    }
}
fn set(t: &mut Vec<i32>) {
    let y = t.get_mut(0).unwrap();
    *y = 7;
}
fn rename(names: &mut Vec<String>) {
    let n = &mut names[0];
    *n = String::from("z");
}
fn main() {
    let mut nums = vec![1, 2];
    bump(&mut nums);
    bump_index(&mut nums);
    maybe(&mut nums);
    set(&mut nums);
    let mut words = vec![String::from("a")];
    rename(&mut words);
    println!("{:?} {:?}", nums, words);
}
