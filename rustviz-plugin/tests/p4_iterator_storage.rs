// IntoIterator may manufacture each item, even when the input owns a String.
struct Ticket(String);

impl IntoIterator for Ticket {
    type Item = String;
    type IntoIter = std::option::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        Some(String::from("new")).into_iter()
    }
}

fn manufactured(ticket: Ticket) {
    for fresh in ticket {
        drop(fresh);
    }
}

// The loan of values survives the move of each first, into the next pass.
fn moved_item(values: &mut Vec<String>) {
    for first in &mut *values {
        drop(first);
        std::hint::black_box(());
    }
}

struct Trace(&'static str);

impl Drop for Trace {
    fn drop(&mut self) {
        println!("{}", self.0);
    }
}

fn early() {
    let outer = [Trace("outer item"), Trace("outer rest")].into_iter();
    for a in outer {
        let inner = [Trace("inner item"), Trace("inner rest")].into_iter();
        for b in inner {
            if std::hint::black_box(true) {
                return;
            }
            drop(b);
        }
        drop(a);
    }
}

fn main() {
    early();
}

fn read_pass(strings: Vec<String>) {
    for word in &strings {
        std::hint::black_box(word);
        std::hint::black_box(());
    }
}

struct Wrapped(Vec<i32>);
impl std::ops::Deref for Wrapped {
    type Target = [i32];
    fn deref(&self) -> &[i32] { Box::leak(Box::new([1])) }
}
impl std::ops::DerefMut for Wrapped {
    fn deref_mut(&mut self) -> &mut [i32] { Box::leak(Box::new([1])) }
}
fn custom_deref() {
    let mut wrapped = Wrapped(vec![17]);
    for item in wrapped.iter_mut() {
        *item = 0;
    }
    assert_eq!(wrapped.0[0], 17);
}
