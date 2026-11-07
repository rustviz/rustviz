fn first_word(s: &String) -> &str {
    &s[..1]
}

fn main() {
    let my_string = String::from("hello world");
    let word = first_word(&my_string);
    println!("{}", word);
}
