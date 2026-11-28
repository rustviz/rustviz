enum Choice<'a> {
    Left { item: (&'a String, u8) },
    Right { item: (&'a String, u8) },
    Empty,
}

fn choose<'a>(a: &'a String, b: &'a String, mode: u8) -> Choice<'a> {
    match mode {
        0 => Choice::Left { item: (a, 0) },
        1 => Choice::Right { item: (b, 0) },
        _ => Choice::Empty,
    }
}

fn construct() {
    let stored = String::from("stored");
    let known = Choice::Left { item: (&stored, 0) };
    std::hint::black_box(known);
}

fn first<'a>(a: &'a String, _: &'a String) -> &'a String {
    a
}

fn repoint<'a>(slot: &mut &'a String, replacement: &'a String) {
    *slot = replacement;
}

struct Slots<'a> {
    first: &'a String,
    second: &'a String,
}

fn repoint_field<'a>(slot: &mut Slots<'a>, replacement: &'a String) {
    slot.first = replacement;
}

fn projected_store() {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let mut slots = Slots { first: &a, second: &b };
    repoint_field(&mut slots, &c);
    assert!(std::ptr::eq(slots.first, &c));
    assert!(std::ptr::eq(slots.second, &b));
}

enum Bag<'a> {
    Occupied(&'a String),
}

impl<'a> Bag<'a> {
    fn inspect(&mut self, _: &'a String) {}
}

fn receiver_constraint() {
    let kept = String::from("kept");
    let inspected = String::from("inspected");
    let mut bag = Bag::Occupied(&kept);
    bag.inspect(&inspected);
    std::hint::black_box(bag);
}

fn swap_values() {
    let a = String::from("a");
    let b = String::from("b");
    let mut first_holder = Some(&a);
    let mut second_holder = Some(&b);
    std::mem::swap(&mut first_holder, &mut second_holder);
    assert!(std::ptr::eq(first_holder.unwrap(), &b));
    assert!(std::ptr::eq(second_holder.unwrap(), &a));
}

fn main() {
    let alpha = String::from("alpha");
    let beta = String::from("beta");
    let gamma = String::from("gamma");
    let only = first(&alpha, &beta);
    assert!(std::ptr::eq(only, &alpha));

    let branch = choose(&alpha, &beta, 1);
    std::hint::black_box(branch);

    let mut left = &alpha;
    let mut right = &beta;
    repoint(&mut left, &gamma);
    std::mem::swap(&mut left, &mut right);
    assert!(std::ptr::eq(left, &beta));
    assert!(std::ptr::eq(right, &gamma));
}
