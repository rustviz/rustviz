fn main() {
    let s = String::from("hi");
    let _x: i32 = loop {
        let _t = &s;
        break 5;
    };
}
