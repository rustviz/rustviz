fn main() {
    let s = String::from("a");
    let mut s2 = s;
    let c = s2.len() > 3;
    let t = s2;
    'blk: {
        if c { break 'blk; }
        s2 = String::from("b");
    }
    println!("{}", t);
}
