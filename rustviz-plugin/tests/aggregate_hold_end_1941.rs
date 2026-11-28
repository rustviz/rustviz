#![allow(dead_code)]

fn read(_: &String) {}

fn array() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let items = [&a, &b];
    let [first, second] = items;
    read(first);
    read(second);
    a.clear();
    b.clear();
}

fn tuple() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let pair = (&a, &b);
    let copy = pair;
    read(copy.0);
    read(copy.1);
    a.clear();
    b.clear();
}

struct Pair<'a, 'b> {
    first: &'a String,
    second: &'b String,
}

fn structure() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let record = Pair { first: &a, second: &b };
    let copy = record;
    read(copy.first);
    read(copy.second);
    a.clear();
    b.clear();
}

fn nested() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut c = String::from("c");
    let nested = ([&a, &b], (&c,));
    let copy = nested;
    read(copy.0[0]);
    read(copy.0[1]);
    read(copy.1.0);
    a.clear();
    b.clear();
    c.clear();
}

fn conditional(flag: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut c = String::from("c");
    let items = [if flag { &a } else { &b }, &c];
    let second = items[1];
    read(second);
    a.clear();
    b.clear();
    c.clear();
}

fn alternatives(flag: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut c = String::from("c");
    let ra = &a;
    let rb = &b;
    let selected;
    if flag {
        b.clear();
        selected = ra;
    } else {
        a.clear();
        selected = rb;
    }
    let pair = (selected, &c);
    let copy = pair;
    read(copy.0);
    read(copy.1);
    a.clear();
    b.clear();
    c.clear();
}

fn overwritten_field(flag: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let mut c = String::from("c");
    let ra = &a;
    let rb = &b;
    let mut pair = (ra, &c);
    if flag {
        pair.0 = rb;
    }
    let copy = pair;
    read(copy.0);
    read(copy.1);
    c.clear();
}

fn element_loans() {
    let data = [String::from("a"), String::from("b")];
    let pair = (&data[0], &data[1]);
    let copy = pair;
    read(copy.0);
    read(copy.1);
}

fn one_loan() {
    let mut a = String::from("a");
    let pair = (&a, 0u8);
    let copy = pair;
    read(copy.0);
    a.clear();
}

fn projected_transfer() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let wrapper = ((&a, &b),);
    let copy = wrapper.0;
    read(copy.0);
    read(copy.1);
    a.clear();
    b.clear();
}

fn branch_stops(flag: bool) {
    let mut a = String::from("a");
    let items = [&a];
    let r = items[0];
    let keep = r;
    if flag {
        read(r);
    } else {
        read(r);
    }
    read(keep);
    a.clear();
}

fn repeated_guard(value: (bool, bool)) {
    let mut a = String::from("a");
    match value {
        (true, _) | (_, true) if {
            let pair = [&a];
            let copy = pair;
            read(copy[0]);
            true
        } => {}
        _ => {}
    }
    a.clear();
}

fn opaque_reborrow(flag: bool) {
    static EMPTY: String = String::new();
    fn fallback(_: &String) -> &String {
        &EMPTY
    }

    let mut a = String::from("a");
    let mut c = String::from("c");
    let ra = &a;
    let source = if flag { ra } else { fallback(ra) };
    let pair = (&*source, &c);
    let copy = pair;
    read(copy.0);
    read(copy.1);
    a.clear();
    c.clear();
}

fn stops_on_the_same_row(flag: bool) {
    let mut a = String::from("a");
    let items = [&a];
    let r = items[0];
    let keep = r;
    if flag { read(r); } else { read(r); }
    read(keep);
    a.clear();
}

fn read_mut(s: &mut String) { s.clear(); }

fn mutable_element_loans() {
    let mut data = [String::from("a"), String::from("b")];
    let [ref mut x, ref mut y] = data;
    let pair = (x, y);
    let moved = pair;
    read_mut(moved.0);
    read_mut(moved.1);
}
