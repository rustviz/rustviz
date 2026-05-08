fn read(_n: i32) {}

fn main() {
    let s = String::from("hi");
    for i in 0..3 {
        read(i);
        let _t = &s;
    }
}
