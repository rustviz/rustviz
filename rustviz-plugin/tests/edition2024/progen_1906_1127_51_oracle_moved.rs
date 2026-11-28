fn split(s: String) -> (String, i32) {
    (s, 1)
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let s = String::from("abc");
    let tp = split(s);
    let Some(x) = maybe(String::from("abc")) else {
        return;
    };
}
