struct P { name: String, age: u32 }
fn grow(s: &mut String) { s.push('!'); }
fn main() {
    let mut p = P { name: String::from("Ann"), age: 30 };
    let r = &mut p;
    r.age += 1;
    r.name.push('!');
    grow(&mut r.name);
    println!("{} {}", p.name, p.age);
}
