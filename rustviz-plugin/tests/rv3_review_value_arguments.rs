fn take<T>(_: T) {}
fn take_first(_: String, _: ()) {}
fn answer() -> usize { 42 }
fn pred(_: i32) -> bool { true }
fn inspect<T>(_: T) {}
fn ignore<T>(_: T) -> std::iter::Once<&'static str> { std::iter::once("other") }

fn guarded(o: Option<i32>, either: Result<(), ()>, shared: &String) {
    match o {
        Some(x) if pred(x) => {}
        _ => {}
    }
    match either {
        Ok(_) | Err(_) if { take(shared); true } => {}
        _ => {}
    }
}

fn caller_drops_second(left: String, right: String) {
    take((left, right).0);
}

fn argument_slots(first: String, second: String) {
    take_first(first, inspect(second));
    take(answer());
}

fn independent_iterator() {
    let mut text = String::from("local");
    let r = &text;
    for _ in ignore(r) {
        text.push('!');
    }
    assert_eq!(text, "local!");
}

fn main() {
    caller_drops_second(String::from("first"), String::from("second"));
    independent_iterator();
}
