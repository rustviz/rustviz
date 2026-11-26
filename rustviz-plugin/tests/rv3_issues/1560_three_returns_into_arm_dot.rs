fn main() {
    let s = String::from("abc");
    let a = &s;
    let r = &s;
    let q = &s;
    if s.len() > 5 {
        println!("{}", q);
    } else if s.len() > 2 {
        println!("{} {} {}", r, a, q);
    } else if s.len() > 1 {
        println!("{}", q);
    } else {
    }
}
