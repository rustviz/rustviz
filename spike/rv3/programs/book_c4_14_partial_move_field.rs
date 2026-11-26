struct Person {
    name: String,
    age: u32,
}

fn main() {
    let p = Person { name: String::from("Ann"), age: 30 };
    let n = p.name;
    println!("{} {}", n, p.age);
}
