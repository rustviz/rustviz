struct Item(String);

fn replace_or_reborrow(first: bool) {
    let mut item = Item(String::from("old"));
    let original = &mut item;
    if first {
        *original = Item(String::from("replacement"));
        item.0.push('a');
    } else {
        let next = &mut item;
        next.0.push('b');
    }
}

fn main() {}
