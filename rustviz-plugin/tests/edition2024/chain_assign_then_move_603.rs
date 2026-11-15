fn take(s: String) -> bool {
    s.len() > 5
}

fn main() {
    let y = Some(String::from("new"));
    let mut z = String::from("old");
    if let Some(a) = y && { z = a; true } && take(z) {
        println!("taken");
    } else {
        println!("not taken");
    }
}
