fn set<'a>(slot: &mut &'a String, v: &'a String) {
    *slot = v;
}
fn main() {
    let a = String::from("a");
    let t = String::from("t");
    let mut refs = vec![&a];
    let mut it = refs.iter_mut();
    set(it.next().unwrap(), &t);
    println!("{}", t);
    println!("{}", refs[0]);
}

fn direct() {
    let a = String::from("a");
    let new_value = String::from("new");
    let mut slots = [&a];
    let mut iterator = slots.iter_mut();
    *iterator.next().unwrap() = &new_value;
    println!("{}", new_value);
    println!("{}", slots[0]);
}

fn index_argument() {
    let a = String::from("a");
    let indexed = String::from("new");
    let mut slots = vec![&a];
    set(&mut slots[0], &indexed);
    println!("{indexed}");
    println!("{}", slots[0]);
}
