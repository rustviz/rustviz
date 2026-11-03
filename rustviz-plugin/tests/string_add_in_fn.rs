fn shout(s: String) -> String {
    s + "!"
}

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let same = a == b;
    let s = shout(a + &b);
    println!("{} {} {}", s, b, same);
}
