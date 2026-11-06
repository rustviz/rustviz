// #136: inherent methods with each kind of receiver, on a tuple
// struct, a generic struct, and called through the type's path.
struct Wrapper { s: String }
struct Counter { count: u32 }
struct Person { name: String }
struct Pair(String, i32);
struct Point<T> { x: T }

impl Wrapper {
    fn into_inner(self) -> String { self.s }
}

impl Counter {
    fn incr(self: &mut Self) { self.count += 1; }
    fn get(self: &Self) -> u32 { self.count }
}

impl Person {
    fn name<'a>(&'a self) -> &'a str { &self.name }
}

impl Pair {
    fn first(&self) -> &String { &self.0 }
}

impl<T> Point<T> {
    fn x(&self) -> &T { &self.x }
}

fn main() {
    let w = Wrapper { s: String::from("hi") };
    let inner = w.into_inner();
    let mut c = Counter { count: 0 };
    c.incr();
    let total = Counter::get(&c);
    let p = Person { name: String::from("Ann") };
    let nm = p.name();
    let pair = Pair(String::from("a"), 1);
    let pt = Point { x: 5 };
    println!("{} {} {} {} {}", inner, total, nm, pair.first(), pt.x());
}
