struct Holder<'a> {
    r: &'a String,
    n: i32,
}

fn take(_s: String) {}

fn take_gen<T>(_t: T) {}

fn id<T>(t: T) -> T {
    t
}

fn longest<'a>(a: &'a String, b: &'a String) -> &'a String {
    if a.len() > b.len() { a } else { b }
}

fn downgrade(s: &mut String) -> &String {
    s.push('!');
    s
}

fn wrap_mut<'a>(s: &'a mut String) -> &'a mut String {
    s
}

fn holder<'a>(s: &'a String) -> Holder<'a> {
    Holder { r: s, n: 0 }
}

fn main() {
    let s = String::from("hey");
    let mut s2 = String::from("hi");
    let mut arr = [std::mem::take(&mut s2), String::from("x")];
    println!("{} {}", s2, s);
    'blk: {
        println!("{} {}", s, s2);
        if s2.len() > 1 { break 'blk; }
        let m = wrap_mut(&mut s2);
        let mut s3 = if s.len() > 1 { String::from("hey") } else { s2.clone() };
    }
    if s2.len() > 1 {
        let r = &s2;
        let h = holder(longest(&s, &s));
        let s4 = 'blk2: {
            if r.is_empty() { break 'blk2 id(s2); }
            String::from("hey")
        };
    } else {
        let r2 = downgrade(&mut s2);
        println!("{}", s);
        if r2.is_empty() {
            let n = s.len();
        }
    }
    println!("{}", s);
    'blk3: {
        take_gen(longest(&s, &s));
        if s.len() > 1 { break 'blk3; }
        let r3 = &s;
    }
    let mut s5 = s.to_string();
}
