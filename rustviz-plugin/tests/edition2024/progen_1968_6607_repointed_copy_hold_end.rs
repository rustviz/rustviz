struct Pair {
    a: String,
    b: String,
}

fn id<T>(t: T) -> T {
    t
}

fn downgrade(s: &mut String) -> &String {
    s
}

fn main() {
    let mut s = String::from("x");
    let s2 = String::from("x");
    let b = Box::new(id(s2));
    let mut r = downgrade(&mut s);
    if b.len() > 2 {
    } else {
        r = &*b;
        let mut n = 0;
        while n < 3 {
            let r3 = r;
            let p = Pair { a: String::from("rust"), b: r3.clone() };
        }
    }
}
