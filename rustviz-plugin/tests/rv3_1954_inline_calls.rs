fn inline_result() -> String {
    let text = (|| String::new())();
    text
}

fn inline_tuple() -> (String, u32) {
    ((|| String::from("text"))(), (|| 7)())
}

fn inline_try() -> Option<String> {
    let text = (|| Some(String::from("text")))()?;
    Some(text)
}

fn mutable_guard(candidate: Option<u8>) -> u8 {
    let mut calls = 0;
    let mut f = |_: u8| { calls += 1; false };
    match candidate {
        Some(n) if f(n) => n,
        _ => calls,
    }
}

fn main() {
    assert_eq!(inline_result(), "");
    assert_eq!(inline_tuple(), (String::from("text"), 7));
    assert_eq!(inline_try().as_deref(), Some("text"));
    assert_eq!(mutable_guard(Some(7)), 1);
    assert_eq!(mutable_guard(None), 0);
}

fn whole_at_pattern() -> String {
    let all @ _ = (|| String::new())();
    all
}

fn copied_into_constructor() -> Option<u32> {
    let result = Some((|| 7_u32)());
    result
}

fn distinct_same_text() -> (String, String) {
    ((|| String::new())(), (|| String::new())())
}

fn check<T>(_: &T) -> bool {
    false
}

fn either_guard(candidate: Option<u8>, flag: bool) -> u32 {
    let calls = std::cell::Cell::new(0);
    let f = || { calls.set(calls.get() + 1); false };
    match candidate {
        Some(_) if if flag { f() } else { check(&f) } => 99,
        _ => calls.get(),
    }
}

#[test]
fn runtime_checks() {
    assert_eq!(whole_at_pattern(), "");
    assert_eq!(copied_into_constructor(), Some(7));
    assert_eq!(distinct_same_text(), (String::new(), String::new()));
    assert_eq!(either_guard(Some(0), false), 0);
    assert_eq!(either_guard(Some(0), true), 1);
}
