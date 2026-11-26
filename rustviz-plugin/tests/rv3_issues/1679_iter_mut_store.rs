fn set<'a>(slot: &mut &'a String, v: &'a String) { *slot = v; }
fn main() {
    let a = String::from("a");
    let t = String::from("t");
    let mut refs = vec![&a];
    let mut it = refs.iter_mut();
    set(it.next().unwrap(), &t);
    println!("{}", t);
    println!("{}", refs[0]);
}
