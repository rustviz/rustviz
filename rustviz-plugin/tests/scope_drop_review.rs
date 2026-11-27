struct Plain(i32);

fn mixed_box(b: Box<Plain>, route: u8) {
    match route {
        0 => drop(b),
        1 => drop(*b),
        _ => {}
    }
}

fn refill(mut a: [String; 2], next: [String; 2], take: bool, replace: bool) {
    if take {
        let [first, _] = a;
        drop(first);
        if replace { a = next; }
    }
}

fn large(a: [String; 65]) {
    let [head, ..] = a;
    drop(head);
}

fn large_empty(a: [String; 65]) {
    let [head, tail @ ..] = a;
}

struct Fields { answer: Result<String, String>, other: String }
fn field_flags(p: Fields, take: bool) {
    if let Ok(value) = p.answer { drop(value); }
    if take { drop(p.other); }
}

struct A { take: String, first: String }
struct B { take: String, second: String }
fn siblings(choice: Result<Box<A>, Box<B>>) {
    match choice {
        Ok(a) => {
            let p = a;
            let x = p.take;
        }
        Err(b) => {
            let p = b;
            let y = p.take;
        }
    }
}

struct Ordered { z: String, moved: String, a: String }
fn early(p: Ordered, plain: Plain, leave: bool) {
    drop(p.moved);
    if leave {
        return;
    }
    drop(plain);
}

fn folded(items: Vec<(String, bool)>) {
    for (s, skip) in items {
        if skip {
            drop(s);
            continue;
        }
        std::hint::black_box(&s);
    }
}

fn unwind(text: String, notify: fn()) {
    notify();
    drop(text);
}

fn main() {}

fn ordinary_helper() {}
fn quit_after_helper() -> ! {
    ordinary_helper();
    std::process::exit(0);
}
fn forwarded_exit(text: String) {
    quit_after_helper();
}
