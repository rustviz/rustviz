fn first_word(s: &String) -> &str { &s[..1] }

fn main() {
    let t = String::from("bye");
    let x = if t.len() > 1 { first_word(&t) } else { "none" };
    let u = String::from("hi");
    let y = if let Some(_c) = u.chars().next() { first_word(&u) } else { "none" };
    let w = String::from("yo");
    let z = match w.len() { 0 => "none", _ => first_word(&w) };
    let v = String::from("hey");
    let n = if x.is_empty() { v.len() } else { 0 } + v.capacity();
    println!("{} {} {} {}", x, y, z, n);
}
