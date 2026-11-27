fn main() {
    let mut s = String::from("rust");
    let s2 = s;
    'blk: {
        if true { break 'blk; }
        s = String::from("rust");
    }
    if true { } else if true { s = String::from("hey"); } else { }
}
