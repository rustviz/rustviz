fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn run(v: Vec<String>, flag: bool) -> Option<usize> {
    let s = String::from("rust");
    let s2 = String::from("hi");
    let o = Some(s);
    for e in v {
        if flag {
            break;
        }
        println!("{}", s2);
        println!("{} {}", s2, e);
    }
    if flag {
        let mut r = &s2;
    } else {
        let s3 = 'blk: {
            if o.is_some() { break 'blk s2; }
            o.unwrap()
        };
        println!("{}", s3);
    }
    let s4;
    if flag {
        s4 = String::from("hey");
    } else {
        s4 = String::from("x");
    }
    let Some(x) = maybe(s4) else {
        let s5 = 'blk2: {
            if flag { break 'blk2 String::from("hey"); }
            String::from("rust")
        };
        return None;
    };
    Some(x.len())
}
