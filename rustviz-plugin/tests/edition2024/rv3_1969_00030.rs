struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn make() -> String {
    String::from("made")
}

fn id<T>(t: T) -> T {
    t
}

fn push_x(s: &mut String) {
    s.push('x');
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let mut s = String::from("rust");
    let mut vr = vec![s.as_str(), "hi"];
    let x = &s[..];
    println!("{:?}", vr);
    let b = Box::new(id(s));
    {
        println!("{}", b);
        s = make();
        for i in 0..2 {
            println!("{}", i);
            let r = longest(&s, &s);
        }
    }
    push_x(&mut s);
    let mut h = holder(longest(&s, &s));
    let m = &mut s;
}
