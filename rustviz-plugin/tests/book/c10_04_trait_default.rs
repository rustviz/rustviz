trait Summary {
    fn summarize(&self) -> String {
        String::from("(Read more...)")
    }
}

struct Article {
    title: String,
}

impl Summary for Article {}

fn main() {
    let a = Article { title: String::from("t") };
    let s = a.summarize();
    println!("{} {}", s, a.title);
}
