fn read(_n: i32) {}

fn main() {
    let s = String::from("hi");
    let mut opt: Option<i32> = Some(0);
    while let Some(x) = opt {
        read(x);
        let _t = &s;
        opt = if x < 2 { Some(x + 1) } else { None };
    }
}
