fn main() {
    let text = String::from("a b c");
    let words: Vec<&str> = text.split(' ').collect();
    let first = words[0];
    let s = String::from("s");
    let mut refs = Vec::new();
    refs.push(&s);
    let last = refs[0];
    println!("{} {} {}", first, last, text);
}
