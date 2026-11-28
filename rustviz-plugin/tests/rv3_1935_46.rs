struct Pair {
    a: String,
    b: String,
}

fn make() -> String {
    String::from("made")
}

fn run(o: Option<String>, flag: bool) -> Option<usize> {
    let mut s = String::from("abc");
    let p = Pair { a: make(), b: s };
    'blk: {
        if flag { break 'blk; }
        s = p.a;
    }
    let Some(x) = o else {
        let mut n = 0;
        return None;
    };
    Some(0)
}
