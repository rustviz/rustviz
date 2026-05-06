// A function returns ownership; the caller binds the returned value.

fn make() -> String {
    let s = String::from("hi");
    s
}

fn main() {
    let r = make();
    println!("{}", r);
}
