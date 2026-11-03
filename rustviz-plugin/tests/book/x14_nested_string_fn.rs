fn shout(s: String) -> String {
    s + "!"
}

fn main() {
    let s = shout(shout(String::from("hi")));
    println!("{}", s);
}
