fn f(s: String, t: String, a: bool, c: bool) {
    let mut r;
    if a {
        r = &s;
        println!("{}", r);
    }
    if !a {
        if c {
            r = &t;
            println!("{}", r);
        } else {
        }
    }
}

fn main() {}
