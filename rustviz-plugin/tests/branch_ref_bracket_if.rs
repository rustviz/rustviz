fn first_word(s: &String) -> &str { &s[..1] }

fn main() {
    let t = String::from("bye");
    let x = if t.len() > 1 { first_word(&t) } else { "none" };
    println!("{}", x);
}
