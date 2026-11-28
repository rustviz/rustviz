fn swap_payload() {
    let a = String::from("a");
    let b = String::from("b");
    let mut slot = Some(&a);
    let mut other = &b;
    if let Some(ref mut payload) = slot {
        std::mem::swap(&mut *payload, &mut other);
        std::hint::black_box(*payload);
    }
    let copied = slot;
    assert!(std::ptr::eq(copied.unwrap(), &b));
    assert!(std::ptr::eq(other, &a));
}

fn swap_matched_payloads() {
    let a = String::from("a");
    let b = String::from("b");
    let mut left = Some(&a);
    let mut right = Some(&b);
    match (&mut left, &mut right) {
        (Some(first), Some(second)) => {
            std::mem::swap(second, first);
            let _ = (*first, *second);
        }
        _ => return,
    }
    let copied_left = left;
    let copied_right = right;
    assert!(std::ptr::eq(copied_left.unwrap(), &b));
    assert!(std::ptr::eq(copied_right.unwrap(), &a));
}

fn swap_whole_holders() {
    let a = String::from("a");
    let b = String::from("b");
    let mut left = Some(&a);
    let mut right = Some(&b);
    std::mem::swap(&mut right, &mut left);
    let copied_left = left;
    let copied_right = right;
    assert!(std::ptr::eq(copied_left.unwrap(), &b));
    assert!(std::ptr::eq(copied_right.unwrap(), &a));
}

fn main() {
    swap_payload();
    swap_matched_payloads();
    swap_whole_holders();
}

fn swap_through_alias() {
    let a = String::from("a");
    let b = String::from("b");
    let mut left = &a;
    let mut right = &b;
    let via = &mut left;
    std::mem::swap(&mut *via, &mut right);
    assert_eq!(via.as_str(), "b");
    assert_eq!(left.as_str(), "b");
    assert_eq!(right.as_str(), "a");
}

fn swap_through_implicit_alias() {
    let a = String::from("a");
    let b = String::from("b");
    let mut left = &a;
    let mut other = &b;
    let alias = &mut left;
    std::mem::swap(alias, &mut other);
    assert_eq!(left.as_str(), "b");
    assert_eq!(other.as_str(), "a");
}

enum A {
    X(u8),
    Y(u8),
}

fn swap_in_guard(pattern: A) {
    let a = String::from("a");
    let b = String::from("b");
    let mut left = &a;
    let mut right = &b;
    match pattern {
        A::X(n) | A::Y(n) if {
            std::mem::swap(&mut left, &mut right);
            n > 1
        } => {}
        _ => {}
    }
    assert_eq!(left.as_str(), "b");
    assert_eq!(right.as_str(), "a");
}

enum Outer<'a> {
    One(Option<&'a String>),
    Empty,
}

fn nested_payload() {
    let a = String::from("a");
    let b = String::from("b");
    let mut outer = Outer::One(Some(&a));
    let mut other = &b;
    if let Outer::One(Some(ref mut payload)) = outer {
        std::mem::swap(&mut *payload, &mut other);
        assert_eq!(payload.as_str(), "b");
    }
    assert_eq!(other.as_str(), "a");
}

fn same_row_sibling(early: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let mut slot = Some(&a);
    let mut other = &b;
    if let Some(ref mut payload) = slot {
        std::mem::swap(&mut *payload, &mut other);
        if early { std::hint::black_box(0); } else { std::hint::black_box(*payload); }
    }
    assert_eq!(other.as_str(), "a");
}

#[test]
fn runtime_checks() {
    swap_payload();
    swap_matched_payloads();
    swap_whole_holders();
    swap_through_alias();
    swap_through_implicit_alias();
    swap_in_guard(A::X(1));
    swap_in_guard(A::Y(2));
    nested_payload();
    same_row_sibling(false);
    same_row_sibling(true);
}
