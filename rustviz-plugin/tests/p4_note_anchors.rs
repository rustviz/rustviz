use std::cell::RefCell;

fn borrow_spelling() {
    let pair = (String::from("x"),);
    let _ = pair /* ref is trivia */ .0.len();
    let explicit = & /* punctuation is HIR */ pair.0;
    let (ref written,) = pair;
    let (inferred,) = &pair;
    assert_eq!(explicit, written);
    assert_eq!(written, inferred);
}

fn guard_notes() {
    let cell = RefCell::new(0);
    if let _ = () { let _ = cell.as_ptr(); } else { let _ = cell.borrow(); }
    let seen = *cell
        .borrow();
    assert_eq!(seen, 0);
}
