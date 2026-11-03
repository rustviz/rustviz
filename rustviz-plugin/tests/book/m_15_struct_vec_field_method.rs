struct Stack {
    items: Vec<i32>,
}

impl Stack {
    fn push(&mut self, x: i32) {
        self.items.push(x);
    }
}

fn main() {
    let mut s = Stack { items: Vec::new() };
    s.push(1);
    println!("{}", s.items.len());
}
