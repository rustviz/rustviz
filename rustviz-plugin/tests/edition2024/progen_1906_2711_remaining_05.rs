fn id<T>(t: T) -> T {
    t
}

fn run(r: &String, o: Option<String>) -> Option<usize> {
    let mut s = String::from("hey");
    let mut b = Box::new(s);
    'blk: {
        s = *b;
        if r.is_empty() { break 'blk; }
        b = Box::new(id(s));
    }
    let Some(mut x2) = o else { return None; };
    Some(0)
}
