struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn len_of(s: &String) -> usize {
    s.len()
}

fn main() {
    let mut s = String::from("x");
    let p = Pair { a: make(), b: s };
    len_of(&p.b);
    println!("{}", p.a);
    if true {
        let Pair { b: r, .. } = &p;
        let s2 = format!("{}!", r);
    } else {
        println!("{}", p.a);
    }
    let mut n = 0;
    while n < 3 {
        n += 1;
        if n > 0 {
            continue;
        }
        let mut tp = (String::from("rust"), 1);
        let Pair { b: ref r2, .. } = p;
    }
}
