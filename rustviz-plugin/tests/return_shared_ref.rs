fn first_word(s: &String) -> &str {
    &s[..1]
}

fn pick(r: &String) -> &String {
    r
}

fn make() -> String {
    let t = String::from("hi");
    t
}

fn main() {
    let owned = make();
    let w = first_word(&owned);
    let p = pick(&owned);
    println!("{} {} {}", owned, w, p);
}
