fn first_word(s: &String) -> &str {
    &s[..1]
}
fn show(s: &String, v: &Vec<i32>) {
    let w = first_word(s);
    let q = v.first().unwrap();
    println!("{} {}", w, q);
}
fn main() {
    let s = String::from("hi");
    let v = vec![1];
    show(&s, &v);
}
