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
    println!("{}", n);
}
