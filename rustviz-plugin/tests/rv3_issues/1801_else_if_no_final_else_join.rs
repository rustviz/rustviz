fn f(s: String, o: Option<String>, a: bool) {
    let r;
    if let Some(x) = &o {
        r = x;
        println!("{}", r);
    } else if a {
        r = &s;
        println!("{}", r);
    }
    println!("{}", s);
}
fn main() { f(String::new(), None, false); }
