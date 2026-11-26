fn run(v: Vec<String>, c: bool, s2: String) {
    if let Some(x) = v.first() {
        if c {
            drop(s2);
            let o = x.clone();
        }
    }
}

fn main() {
    run(vec![String::from("a")], true, String::from("b"));
}
