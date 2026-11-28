fn main() {
    let o: Option<String> = None;
    if o.is_some() {
    } else {
        let Some(x2) = &o else {
            return;
        };
    }
}
