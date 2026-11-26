struct Store<'a> {
    items: Vec<&'a String>,
}
impl<'a> Store<'a> {
    fn store(&mut self, x: &'a mut String, y: &'a String) {
        self.items.push(x);
        self.items.push(y);
    }
}
fn first<'a>(s: &Store<'a>) -> &'a String {
    s.items[0]
}
fn main() {
    let mut a = String::from("a");
    let b = String::from("b");
    let mut v = Store { items: Vec::new() };
    let mut w: Vec<&String> = Vec::new();
    v.store(&mut a, &b);
    w.push(first(&v));
    println!("{:?}", w);
}
