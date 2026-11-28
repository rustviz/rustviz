fn main() {
    let s = String::from("hey");
    let mut s2 = s;
    let arr = [s2, String::from("hey")];
    if true {
        'blk: {
            break 'blk;
        }
    } else {
        s2 = String::from("hey");
    }
    println!("{:?}", arr);
}
