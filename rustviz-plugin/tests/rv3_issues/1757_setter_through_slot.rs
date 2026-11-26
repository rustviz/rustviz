fn slot<'s, 'a>(r: &'s mut &'a String) -> &'s mut &'a String { r }
fn set<'a>(dst: &mut &'a String, src: &'a String) { *dst = src; }
fn main() {
    let a = String::new(); let mut t = String::new();
    let mut r = &a;
    set(slot(&mut r), &t);
    // t.clear();
    println!("{r}");
}
