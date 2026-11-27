fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let mut s = String::from("rust");
    let o = maybe(s.clone());
    match o {
        Some(ref x) => {
            if x.is_empty() {
                return;
            }
            let m = &mut s;
        }
        None => {}
    }
}
