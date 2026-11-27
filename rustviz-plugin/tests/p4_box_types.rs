fn different_types() {
    { let b = Box::new(String::from("first")); drop(*b); }
    { let b = Box::new(vec![1u8]); drop(b.len()); }
}
