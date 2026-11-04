trait Summary {
    fn summarize(&self) -> String;
}

struct Tweet {
    username: String,
    content: String,
}

impl Summary for Tweet {
    fn summarize(&self) -> String {
        format!("{}: {}", self.username, self.content)
    }
}

fn main() {
    let t = Tweet { username: String::from("horse"), content: String::from("hi") };
    println!("1 new tweet: {}", t.summarize());
}
