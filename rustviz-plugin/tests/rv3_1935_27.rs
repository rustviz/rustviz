fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let s = String::from("hi");
    let o = Some(format!("{}!", s));
    let Some(x) = maybe(String::from("x")) else {
        return;
    };
    let s2 = match o {
        Some(y) => y,
        None => String::new(),
    };
}
