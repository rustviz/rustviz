// #250: `self.items` has no column of its own, but the argument of
// `self.items.push(item)` is still moved into `push`, so `item` has
// nothing left to drop when `add` returns. `&t` is borrowed by
// `push_str` the same way.

struct Stack {
    items: Vec<String>,
    name: String,
}

impl Stack {
    fn add(&mut self, item: String, t: &String) {
        self.items.push(item);
        self.name.push_str(t);
    }
}

fn main() {
    let mut st = Stack { items: Vec::new(), name: String::new() };
    let s = String::from("x");
    let t = String::from("y");
    st.add(s, &t);
}
