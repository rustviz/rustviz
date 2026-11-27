fn split(s: String) -> (String, i32) {
    (s, 1)
}

fn main() {
    let tp = split(String::from("rust"));
    drop(tp);
}
