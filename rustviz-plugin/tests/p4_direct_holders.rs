#![allow(dead_code, unused_assignments)]

struct Pair<'a> {
    left: &'a String,
    right: &'a String,
}

fn read(_: &String) {}

fn siblings() {
    let mut same = String::from("same");
    let other = String::from("other");
    let mut pair = Pair { left: &same, right: &same };
    read(pair.left);
    pair.left = &other;
    read(pair.right);
    read(pair.left);
    same.clear();
}

fn overwrite() {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let original = &a;
    let mut pair = (original, &b);
    pair.0 = original; pair.0 = &c;
    read(pair.0);
}

fn array_move() {
    let mut value = String::from("value");
    let mut other = String::from("other");
    let items = [&value, &other];
    let [first, second] = items;
    read(first);
    read(second);
    value.clear();
    other.clear();
}

fn shared_copy() {
    let mut value = String::from("value");
    let exclusive = &mut value;
    let shared = &*exclusive;
    let pair = (shared,);
    let copy = pair.0;
    read(copy);
    exclusive.clear();
}

fn shared_from_mutable_origin() {
    let mut value = String::from("value");
    let shared: &String = &mut value;
    let slots = [shared];
    let [copy] = slots;
    read(copy);
    value.clear();
}

fn match_elements() {
    let x = String::from("x");
    let y = String::from("y");
    let items = [&x, &y];
    match items {
        [first, second] => {
            read(first);
            read(second);
        }
    }
}

struct Wrap<'a> {
    slots: [&'a String; 2],
}

fn field_array() {
    let a = String::from("a");
    let b = String::from("b");
    let wrap = Wrap { slots: [&a, &b] };
    let [r, s] = wrap.slots;
    read(r);
    read(s);
}

fn fixed_index() {
    let a = String::from("a");
    let b = String::from("b");
    let slots = [&a, &b];
    let r = slots[0];
    read(r);
}

fn payload_restriction() {
    let value = String::from("value");
    let pack = Some(String::from("payload"));
    let Some(ref payload) = pack else { return };
    let slots = [&value, payload];
    let [r, s] = slots;
    read(r);
    read(s);
}

fn staged_constant() {
    let x = String::from("x");
    let pair = (("hello", &x),);
    assert_eq!(pair.0.0, "hello");
    read(pair.0.1);
}

fn overwrite_element() {
    let x = String::from("x");
    let y = String::from("y");
    let mut arr = [&x, &x];
    arr[0] = &y;
    read(arr[0]);
    read(arr[1]);
}

fn caller_reaches_join<'a>(mut r: &'a String, pair: &'a (String,), flag: bool) {
    if flag { r = &pair.0; }
    read(r);
    r = &pair.0;
    read(r);
}
