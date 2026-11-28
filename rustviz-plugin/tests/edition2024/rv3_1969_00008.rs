fn take_gen<T>(_t: T) {}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn downgrade(s: &mut String) -> &String {
    s.push('!');
    s
}

fn main() {
    let mut s = String::from("ab");
    let mut s2 = s.to_uppercase();
    let v = vec![s.clone(), String::from("hi")];
    let mut r = v.first().unwrap();
    take_gen(v.last().unwrap());
    r = downgrade(&mut s);
    if v.len() > 1 {
        if s2.len() > 1 {
            let n = s.len();
            if s.len() > 1 { return; }
        }
        std::mem::swap(&mut s, &mut s2);
        if s.len() > 1 {
            println!("{:?} {}", v, s);
        } else {
            println!("{}", s);
            s2.push('!');
        }
    } else {
        println!("{} {}", r, s2);
        'blk: {
            if r.is_empty() { break 'blk; }
            let m = &mut s2;
            let r2 = &s;
        }
    }
    println!("{:?}", v);
}
