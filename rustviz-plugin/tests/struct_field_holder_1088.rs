// #1088: a struct field that isn't a reference but holds one
// (`items: Vec<&'a str>`) borrows what it holds, like `let items =
// vec![w.as_str()];` would: w stays borrowed while shelf.items, or a
// reference copied out of it (`first`, `b`), is in use.
struct Shelf<'a> {
    items: Vec<&'a str>,
}

impl<'a> Shelf<'a> {
    fn new(items: Vec<&'a str>) -> Shelf<'a> {
        Shelf { items }
    }

    fn top(&self) -> &'a str {
        self.items[0]
    }
}

// Two lifetimes: `first` returns only `x`'s.
struct Pair<'a, 'b> {
    x: &'a str,
    y: &'b str,
}

impl<'a, 'b> Pair<'a, 'b> {
    fn first(&self) -> &'a str {
        self.x
    }
}

fn main() {
    let w = String::from("book");
    let shelf = Shelf { items: vec![w.as_str()] };
    let n = shelf.items.len();
    println!("{} {}", n, w);
    let first = shelf.items[0];
    println!("{}", first);
    let b = shelf.top();
    println!("{}", b);

    let v = String::from("pen");
    let other = Shelf::new(vec![v.as_str()]);
    let c = other.items[0];
    println!("{}", c);

    let a = String::from("a");
    let z = String::from("z");
    let p = Pair { x: a.as_str(), y: z.as_str() };
    let f = p.first();
    drop(z);
    println!("{}", f);
}
