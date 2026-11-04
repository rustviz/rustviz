fn main() {
    let greeting_text = String::from("hello");
    let greeting_reference = &greeting_text;
    let n = 1;
    println!("{} {}", greeting_reference, n);
}
