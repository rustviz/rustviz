struct Pair {
    a: String,
    b: String,
}

fn id<T>(t: T) -> T {
    t
}

fn mut_gen<T>(_t: &mut T) {}

fn run(r: &String) -> Option<usize> {
    let s = String::from("hey");
    let mut p = Pair { a: id(s), b: r.to_string() };
    let m = &mut p.b;
    let s2 = 'blk2: {
        if r.is_empty() { break 'blk2 m.to_string(); }
        String::from("abc")
    };
    mut_gen(m);
    Some(0)
}
