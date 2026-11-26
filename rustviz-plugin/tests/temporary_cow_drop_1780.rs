// A temporary Cow is often Cow::Borrowed, which owns nothing.
fn unbound_cow() {
    String::from_utf8_lossy(b"hi");
    let n = 1;
    println!("{n}");
}

fn unbound_option() {
    Some(String::new());
    let n = 1;
    println!("{n}");
}

fn let_extended_cow() {
    let c = &String::from_utf8_lossy(b"hi");
    println!("{c}");
}

fn literal_cow() {
    let (x, _) = (String::new(), String::from_utf8_lossy(b"a"));
    println!("{x}");
}
