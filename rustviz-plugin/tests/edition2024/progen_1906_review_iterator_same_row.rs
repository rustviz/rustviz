fn show(s: &String) { assert!(!s.is_empty()); }

fn run(c: bool, mut xs: Vec<String>) {
    for x in &xs {
        if c { show(x); } else { show(x); } }
    xs.clear();
}
