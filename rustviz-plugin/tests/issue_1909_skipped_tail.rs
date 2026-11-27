fn main() {
    let tp = (String::from("hi"), 1);
    let s3 = 'blk: {
        if true { break 'blk String::from("x"); }
        tp.0
    };
    println!("{}", s3);
}
