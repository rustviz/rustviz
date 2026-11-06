// #136: the one-field accessor that used to crash the plugin (#74).
struct Holder { n: i32 }

impl Holder {
    fn get(&self) -> i32 { self.n }
}

fn main() {
    let h = Holder { n: 5 };
    let v = h.get();
    println!("{}", v);
}
