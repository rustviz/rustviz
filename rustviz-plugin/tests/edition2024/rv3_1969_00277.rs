struct Pair {
    a: String,
    b: String,
}

fn run(flag: bool) -> Option<usize> {
    let s = String::from("ab");
    let s2 = s;
    let mut p = Pair { a: String::from("hi"), b: s2 };
    drop(p);
    if flag {
        if flag {
            let o = Some(String::from("abc"));
            let _ = o;
        } else {
            p = Pair { a: String::from("rust"), b: String::from("ab") };
            println!("{}", p.a);
        }
        if true { return None; }
    } else {
        p = Pair { a: String::from("hey"), b: String::from("x") };
    }
    let arr = [String::from("rust"), String::from("abc")];
    Some(0)
}
