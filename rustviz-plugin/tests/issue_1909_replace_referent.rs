fn replace(m: &mut String, s: String) {
    *m = s;
}

fn replace_box() {
    let mut b = Box::new(String::from("old"));
    *b = String::from("new");
}
