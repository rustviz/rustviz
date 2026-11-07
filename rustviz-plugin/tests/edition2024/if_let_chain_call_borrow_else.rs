fn main() {
    let mut x = Some(String::from("a"));
    let y = Some(1);
    if let Some(a) = x.as_mut() && let Some(b) = y {
        a.push('!');
        println!("{b}");
    } else {
        println!("no");
    }
}
