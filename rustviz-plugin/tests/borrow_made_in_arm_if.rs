fn main() {
    let y = 9;
    let x = 8;
    let mut c = &x;
    let flag = true;
    if flag {
        c = &y;
    } else {
        println!("{}", c);
    }
}
