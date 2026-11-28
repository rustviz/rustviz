struct Holder<'a> {
    r: &'a String,
    n: i32,
}

struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn make_pair() -> Pair {
    Pair { a: make(), b: make() }
}

fn id<T>(t: T) -> T {
    t
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn run(mut s: String, r: &String, flag: bool) -> Option<usize> {
    let mut s2 = String::from("hey");
    let s3 = make();
    let mut o: Option<String> = None;
    let mut s4 = format!("{}!", s2);
    {
        println!("{} {:?}", s2, o);
        let m = &mut s4;
        s4.push('!');
        let mut s5 = s3.clone();
    }
    if flag {
        match s2.as_str() {
            "hi" | "ab" => {
                let n = s.len();
            }
            _ => {
                s.push('!');
            }
        }
        let mut h = Holder { r: r, n: 1 };
        if s.len() > 1 {
            println!("{}", h.r);
            h.n += 1;
            Pair { a: s2, b: s4 } = make_pair();
        } else if flag {
            let s6 = String::from("rust");
            let s7 = std::mem::replace(&mut s2, String::new());
        } else {
            println!("{} {}", s4, h.r);
            let s8 = maybe(id(s4))?;
        }
    }
    let s2 = &s2;
    Some(r.len())
}
