struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn id<T>(t: T) -> T {
    t
}

fn len_of(s: &String) -> usize {
    s.len()
}

fn push_x(s: &mut String) {
    s.push('x');
}

fn run(mut s: String, m: &mut String, o: Option<String>, flag: bool) -> Option<usize> {
    let mut s2 = id(s);
    let mut s3 = String::from("hi");
    let mut p = Pair { a: s2.clone(), b: make() };
    if let Some(x2) = &o && x2.len() > 1 && o.is_some() {
        println!("{} {:?}", p.a, o);
    }
    let p2 = Pair { a: s2, b: String::from("ab") };
    let r = &p.b;
    push_x(m);
    if flag {
        let Pair { a: s4, .. } = p2;
        let c = || s3.len();
    } else if s3.len() > 1 {
        let mut b = Box::new(s3);
    } else {
        *m = s3;
        if flag {
            println!("{:?}", o);
            p = Pair { a: String::from("ab"), b: o.unwrap() };
            println!("{}", p2.a);
        } else if r.is_empty() {
            let m2 = m;
            let mut s5 = o?;
            p = Pair { a: format!("{}!", r), b: String::from("ab") };
        } else {
            p.b = p.a;
        }
    }
    p = Pair { a: String::from("ab"), b: String::from("rust") };
    p.a = String::from("hey");
    len_of(&p.b);
    Some(0)
}
