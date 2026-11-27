pub fn three(t: (u8, String, String, String)) {
    match t {
        (0, x, y, _) | (1, y, x, _) | (_, _, x, y) => println!("{x}{y}"),
    }
}

pub fn parent(flag: bool) {
    let t = (flag, (String::from("a"), String::from("b")), (String::from("c"), String::from("d")));
    match t {
        (true, (x, _), y) | (false, y, (x, _)) => println!("{x}{}", y.1),
    }
}

pub fn temporary(flag: bool, a: String, b: String) {
    match (flag, a, b) {
        (true, x, y) | (false, y, x) => println!("{x}{y}"),
    }
}

pub fn reused(flag: bool) {
    {
        let t = (flag, String::from("one"), String::from("two"));
        match t {
            (true, x, y) | (false, y, x) => println!("{x}{y}"),
        }
    }
    {
        let t = (flag, String::from("three"), String::from("four"));
        drop(t.2);
        println!("{}", t.1);
    }
}

pub fn temporary_parent(flag: bool) {
    let a = (String::from("a"), String::from("b"));
    let b = (String::from("c"), String::from("d"));
    match (flag, a, b) {
        (true, (x, _), y) | (false, y, (x, _)) => println!("{x}{}", y.1),
    }
}
