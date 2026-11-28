struct Pair {
    a: String,
    b: String,
}

fn main() {
    let s = String::from("hey");
    let mut p = Pair { a: s.to_string(), b: s };
    drop(p);
    if true {
        'blk: {
            if true { break 'blk; }
            p = Pair { a: String::from("hey"), b: String::from("hi") };
        }
    }
}
