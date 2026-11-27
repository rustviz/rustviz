use std::{cell::Cell, fmt};

struct Held<'a> { item: &'a String }

fn choose_mut<'a>(a: &'a mut String, b: &'a mut String) -> &'a mut String {
    if a.len() > b.len() { a } else { b }
}
fn wrap(item: &String) -> Held<'_> { Held { item } }

struct Pair<'a> { left: &'a String, right: &'a String }
fn get<'a, 'b>(pair: &'a Pair<'b>) -> &'a Pair<'b> { pair }
fn select<'a>(pair: &Pair<'a>, flag: bool) -> &'a String {
    if flag { pair.left } else { pair.right }
}

fn shadow<'a>(x: &'a String, y: &'a String, flag: bool) -> &'a String {
    if flag {
        let x = y;
        x
    } else {
        x
    }
}

fn reassigned<'a>(mut x: &'a String, y: &'a String, flag: bool) -> &'a String {
    x = y;
    if flag { x } else { y }
}

fn carried<'a>(x: &'a str, h: Held<'a>, flag: bool) -> &'a str {
    if flag { x } else { "literal" }
}

struct Counter(i32);
impl Counter {
    fn save<'a>(&'a mut self, out: &mut Option<&'a mut Self>) { *out = Some(self); }
}

struct Sink(String);
impl Sink {
    fn write_fmt(self, args: fmt::Arguments<'_>) -> fmt::Result {
        println!("{}{}", self.0, args);
        Ok(())
    }
}

fn apply(f: impl FnOnce()) { f() }

fn main() {
    let mut left = String::from("left");
    let mut right = String::from("right");
    let held = wrap(choose_mut(&mut left, &mut right));
    println!("{}", held.item);
    let pair = Pair { left: &left, right: &right };
    let picked = select(get(&pair), true);
    println!("{}", picked);
    println!("{}", shadow(&left, &right, true));
    println!("{}", reassigned(&left, &right, true));
    println!("{}", carried(&left, Held { item: &right }, false));

    let mut counter = Counter(0);
    let r = &mut counter;
    let mut out = None;
    r.save(&mut out);
    println!("{}", out.unwrap().0);

    let mut n = 1;
    let p = &mut n;
    apply(|| *p += 1);
    let c = Cell::new(0);
    apply(|| c.set(2));
    let dst = Sink(String::from("output"));
    write!(dst, "{}", c.get()).unwrap();
    let picked_path = restricted(&left, &right, c.get());
    println!("{picked_path}");
}

// Borrowck retains the b arm in its graph. No integer can reach that arm.
fn restricted<'a>(a: &'a String, b: &'a String, n: i32) -> &'a String {
    if n > 2 { a } else if n > 5 { b } else { a }
}
