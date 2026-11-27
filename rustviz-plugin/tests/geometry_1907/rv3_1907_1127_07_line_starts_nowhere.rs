struct Pair {
    a: String,
    b: String,
}

fn main() {
    let p = Pair { a: String::from("rust"), b: String::from("hi") };
    let Pair { a: s2, .. } = p;
    let mut s3 = String::from("rust");
    'blk: {
        if s3.len() > 1 { break 'blk; }
        'blk2: {
            s3 = p.b;
        }
    }
}
