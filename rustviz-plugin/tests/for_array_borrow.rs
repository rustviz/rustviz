fn read(_r: &i32) {}

fn main() {
    let s = String::from("hi");
    let arr = [1i32, 2, 3];
    for x in &arr {
        read(x);
        let _t = &s;
    }
}
