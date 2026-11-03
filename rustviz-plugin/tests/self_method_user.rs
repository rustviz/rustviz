struct Person {
    name: String,
}

impl Person {
    fn into_name(self) -> String {
        self.name
    }
}

fn main() {
    let p = Person { name: String::from("Ann") };
    let n = p.into_name();
    let v = vec![1, 2, 3];
    let total: i32 = Vec::into_iter(v).sum();
    println!("{} {}", n, total);
}
