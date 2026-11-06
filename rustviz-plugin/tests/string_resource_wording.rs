fn takes(s: String) -> usize {
    s.len()
}

fn main() {
    let s = String::from("hi");
    let n = takes(s);
    let t = String::from("bye");
}
