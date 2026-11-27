#[derive(Debug)]
struct Pair { left: String, right: String }

fn field(pair: &mut Pair) -> &mut String {
    let alias = &mut pair.left;
    &mut *alias
}

fn erased<F: FnOnce() -> String>(f: F) -> String {
    f()
}

fn conditional(take: bool) {
    let held = String::from("capture");
    let f = move || drop(held);
    if take { f(); }
}

fn alternatives(take: bool) -> String {
    let held = String::from("capture");
    let f = move || {
        if take { held } else { drop(held); String::from("fresh") }
    };
    f()
}

fn main() {
    let mut pair = Pair { left: String::new(), right: String::new() };
    field(&mut pair).push('a');
    let offset = 1;
    let add = move |x: i32| x + offset;
    let value = add(5);
    let mut count = 0;
    let mut tick = || { count += 1; };
    tick();
    let captured = String::from("moved");
    let take = move || captured;
    let result = take();
    let value2 = erased(|| String::from("opaque"));
    conditional(false);
    println!("{value} {count} {result} {value2} {}", alternatives(true));
}

fn read_slot<'a>(slot: &mut &'a String, write: impl FnOnce(&mut &'a String)) -> &'a String {
    write(slot);
    *slot
}

fn loaded_after_call<'a>(slot: &mut &'a String, replacement: &'a String) -> &'a String {
    read_slot(slot, |s| *s = replacement)
}

fn literal_choice<'a>(first: &'a String, second: &'a String) -> &'a String {
    if false { first } else { second }
}

fn literal_forward<'a>(first: &'a String, second: &'a String) -> &'a String {
    literal_choice(first, second)
}
