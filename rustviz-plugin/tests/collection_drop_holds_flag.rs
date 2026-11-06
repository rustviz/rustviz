fn take(_v: Vec<String>) {}

fn main() {
    let words = vec![String::from("a")];
    let maybe = Some(String::from("b"));
    if words.len() > 5 {
        take(words);
    }
    if maybe.is_none() {
        drop(maybe);
    }
}
