fn read(_n: i32) {}

fn main() {
    let s = String::from("hi");
    let mut n = 0;
    while n < 3 {
        read(n);
        let _t = &s;
        n += 1;
    }
}
