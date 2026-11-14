enum Message {
    Quit,
    Move { x: i32, y: i32 },
    Write(String),
    ChangeColor(i32, i32, i32),
}

impl Message {
    fn call(&self) {}
}

fn main() {
    let m = Message::Write(String::from("hello"));
    m.call();
    let mv = Message::Move { x: 1, y: 2 };
    match mv {
        Message::Move { x, y } => {
            let _sum = x + y;
        }
        _ => {}
    }
}
