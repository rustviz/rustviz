fn set<'a>(slot: &mut Option<&'a String>, v: &'a String) { *slot = Some(v); }
fn main() {
    let t = String::new();
    let mut slots: [Option<&String>; 1] = [None];
    set(&mut slots[0], &t);
    println!("{t}");
    println!("{:?}", slots[0]);
}
