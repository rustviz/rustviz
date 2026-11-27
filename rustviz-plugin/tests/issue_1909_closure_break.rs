fn main() {
    let s = String::from("hi");
    let c = move || s;
    let s2 = 'blk: {
        if true { break 'blk c(); }
        String::from("x")
    };
    let arr = [s2];
    println!("{:?}", arr);
}
