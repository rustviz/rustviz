// #1367 in a let chain (edition 2024): m takes make's String when the
// pattern matches; otherwise it is dropped with the temporary.
fn make() -> String {
    String::from("m")
}

fn let_chain(k: bool, z: Option<String>) {
    if k && let (Some(c), m) = (z, make()) {
        println!("{} {}", c, m);
    }
}

fn main() {
    let_chain(true, None);
}
