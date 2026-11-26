struct P { q: Box<String> }
fn replace(mut p: P) {
    *p.q = String::from("new");
}
