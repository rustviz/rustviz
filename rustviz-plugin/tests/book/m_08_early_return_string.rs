fn pick(flag: bool, s: String) -> String {
    if flag {
        return s;
    }
    String::from("other")
}

fn main() {
    let s = String::from("mine");
    let r = pick(true, s);
    println!("{}", r);
}
