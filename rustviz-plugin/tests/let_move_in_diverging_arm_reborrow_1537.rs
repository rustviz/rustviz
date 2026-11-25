fn main() {
    let mut s = String::from("a");
    let m = &mut s;
    let r = &*m;
    let n = 3;
    if n > 5 {
        let m2 = m;
        return;
    }
    println!("{}", r);
}
