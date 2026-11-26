fn or_pattern(t8: (String, String, bool), c: bool) {
    match t8 {
        (ref a8, _, true) | (_, ref a8, false) => {
            if c { return; }
        }
    }
}
fn main() { or_pattern((String::new(), String::new(), true), true); }
