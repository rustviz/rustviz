struct Fields {
    text: String,
}

fn main() {
    let mut b = Box::new(0);
    if std::hint::black_box(true) {
        *b = 1;
    } else {
        *b = 2;
    }
    assert_eq!(*b, 1);

    let mut boxed = Box::new(String::from("old"));
    match std::hint::black_box(true) {
        true => *boxed = String::from("first"),
        false => *boxed = String::from("second"),
    }

    let mut values = vec![String::from("old")];
    if let Some(value) = std::hint::black_box(Some(String::from("new"))) {
        values[0] = value;
    } else {
        values[0] = String::from("fallback");
    }

    let mut fields = Fields { text: String::new() };
    let field_ref = &mut fields;
    if std::hint::black_box(true) {
        field_ref.text = String::from("first");
    } else {
        field_ref.text = String::from("second");
    }
    assert!(!field_ref.text.is_empty());

    // Writing inside the Box must still be distinguished from rebinding it.
    let replacement;
    if std::hint::black_box(true) {
        replacement = Box::new(String::from("first"));
    } else {
        replacement = Box::new(String::from("second"));
    }
    drop(replacement);
}
