enum Message {
    Quit,
    Write(String),
}

impl Message {
    fn call(&self) {
        match self {
            Message::Quit => println!("quit"),
            Message::Write(s) => println!("{}", s),
        }
    }
}

fn main() {
    let m = Message::Write(String::from("hello"));
    m.call();
    let q = Message::Quit;
    q.call();
}
