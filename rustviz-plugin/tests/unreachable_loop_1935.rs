fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    loop {
        break;
        let o = maybe(String::from("ab"));
        let o2 = maybe(String::from("abc"));
        println!("{:?} {:?}", o, o2);
    }
}
