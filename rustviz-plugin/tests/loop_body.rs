fn read(_n: i32) {}

fn main() {
    let s = String::from("hi");
    let mut n = 0;
    loop {
        read(n);
        let _t = &s;
        n += 1;
        if n >= 3 {
            break;
        }
    }
}
