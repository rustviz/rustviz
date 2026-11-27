#![allow(unreachable_patterns)]

fn take<T>(_: T) {}

pub fn shadowed(n: i32, owned: String, count: i32) {
    match n {
        _ => {}
        1 => {
            take(Some(owned));
            take(Some((count, count)));
        }
    }
}
