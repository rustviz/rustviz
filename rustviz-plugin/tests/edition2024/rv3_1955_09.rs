fn make() -> String {
    String::from("made")
}

fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let s = String::from("abc");
    let v = vec![make(), String::from("ab")];
    let Some(mut x) = maybe(s) else {
        return;
    };
    let c = move || x;
    if let Some(x2) = v.first() {
        if x2.is_empty() && let Some(x3) = v.first() {
            x = String::from("rust");
        }
    }
}
