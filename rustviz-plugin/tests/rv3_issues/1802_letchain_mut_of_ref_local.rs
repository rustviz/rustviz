fn reset(r: &mut &i32) -> bool { *r = &7; false }
pub fn f(o: Option<u8>) {
    let a = 1;
    let mut r = &a;
    if let Some(_n) = o && reset(&mut r) {
        println!("yes");
    } else {
        println!("{r}");
    }
}
