fn len_of(s: &String) -> usize {
    s.len()
}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn run(mut s: String, m: &mut String, flag: bool) -> Option<usize> {
    let mut s2 = String::from("hey");
    let mut s3 = String::from("x");
    let mut b = Box::new(String::from("hey"));
    if s3.len() > 1 {
        let x = first(&s);
        {
            println!("{} {}", x, s);
            println!("{}", s);
        }
    }
    {
        let mut b2 = Box::new(String::from("ab"));
        let m2 = &mut *m;
        let s4 = if s3.len() > 1 { s3 } else { *b2 };
        if flag {
            drop(b);
            println!("{}", s4);
        } else if m2.len() > 2 {
            b2 = Box::new(String::from("hi"));
            len_of(&s2);
            *b2 = String::from("x");
        } else {
            s = String::from("hey");
            let mut c = || s.push('!');
            let r = &b;
        }
    }
    let s5 = 'blk: {
        if flag { break 'blk s2; }
        String::from("hi")
    };
    s3 = std::mem::replace(m, String::new());
    let Some(x2) = maybe(String::from("hi")) else { return None; };
    Some(m.len())
}
