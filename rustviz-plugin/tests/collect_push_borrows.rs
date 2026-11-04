fn main() {
    let text = String::from("a b c");
    let words: Vec<&str> = text.split(' ').collect();
    let s = String::from("s");
    let mut refs = Vec::new();
    refs.push(&s);
    println!("{:?} {:?}", words, refs);
}
