struct Pair {
    a: String,
    b: String,
}

fn id<T>(t: T) -> T {
    t
}

fn main() {
    let s3 = 'blk2: {
        String::from("hey")
    };
    let s5 = id(s3);
    if s5.len() > 1 {
        let p2 = Pair { a: s5, b: String::from("x") };
        match p2 {
            Pair { a: a, b: b } => {
            }
        }
    }
}
