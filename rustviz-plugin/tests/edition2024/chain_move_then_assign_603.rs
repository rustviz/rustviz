fn take(s: String) -> bool {
    s.len() > 2
}

fn main() {
    let w = Some(1);
    let y = Some(String::from("new"));
    let mut z = String::from("old");
    if let Some(_n) = w && take(z) && let Some(a) = y && { z = a; z.len() > 5 } {
        println!("{}", z);
    } else {
        println!("no");
    }
}
