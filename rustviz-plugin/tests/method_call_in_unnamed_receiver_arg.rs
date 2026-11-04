// #249: a method call inside the arguments of a call whose receiver
// has no column (`self.items`, or a temporary in a chain) used to
// panic in annotated_source_gen.rs: the visitor never registered
// `clone` / `as_str` because it skipped those arguments.

struct Stack {
    items: Vec<String>,
}

impl Stack {
    fn add(&mut self, s: &String) {
        self.items.push(s.clone());
    }
}

fn main() {
    let mut st = Stack { items: Vec::new() };
    let s = String::from("x");
    st.add(&s);
    let mut v = vec![String::new()];
    v.last_mut().unwrap().push_str(s.as_str());
}
