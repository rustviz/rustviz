struct Stats { samples: Vec<i32> }

impl Stats {
    fn add(mut self, x: i32) -> Stats {
        self.samples.push(x);
        self
    }
}

fn main() {
    let st = Stats { samples: Vec::new() };
    let done = st.add(1);
}
