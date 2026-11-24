// #1434: a `for` pattern that leaves parts of each element unbound (`_`,
// `..`, a struct pattern's omitted fields): Rust drops them at the end of
// each pass, with the rest of that pass's element. No tooltip said so.
struct P {
    name: String,
    tag: String,
    n: i32,
}

fn main() {
    let firsts = vec![(String::from("f"), String::from("g"))];
    for (f, _) in firsts {
        println!("{}", f);
    }
    let w = vec![String::from("w")];
    for _ in w {
        println!("pass");
    }
    let y = vec![(String::from("y"), 1, String::from("z"))];
    for (_, k, ..) in y {
        println!("{}", k);
    }
    let ps = vec![P { name: String::from("p"), tag: String::from("t"), n: 1 }];
    for P { name, .. } in ps {
        println!("{}", name);
    }
    let v = vec![(String::from("a"), String::from("b"))];
    let mut it = v.into_iter();
    it.next();
    for (a, _) in it {
        println!("{}", a);
    }
    let cp = vec![(1, String::from("c"))];
    for (c, _) in &cp {
        println!("{}", c);
    }
    let nums = vec![(1, 2)];
    for (n, _) in nums {
        println!("{}", n);
    }
}
