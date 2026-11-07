fn first_word(s: &String) -> &str { &s[..1] }

fn main() {
    let a = String::from("apple");
    let b = String::from("berry");
    let n = 2;
    let w = match n {
        1 => first_word(&a),
        _ => first_word(&b),
    };
    println!("{}", w);
    println!("{} {}", a, b);
}
