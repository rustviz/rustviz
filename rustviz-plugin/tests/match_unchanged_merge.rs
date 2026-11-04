// #179: the merge dot of a variable that no branch moves (a match on
// `&opt`, a match on `self` in a `&self` method) used to have an empty
// tooltip.
enum Message { Quit, Write(String) }

impl Message {
    fn call(&self) {
        match self {
            Message::Quit => println!("quit"),
            Message::Write(s) => println!("{}", s),
        }
    }
}

fn main() {
    let opt = Some(String::from("hi"));
    match &opt {
        Some(s) => println!("{}", s),
        None => println!("none"),
    }
    let m = Message::Write(String::from("hello"));
    m.call();
    println!("{:?}", opt);
}
