fn siblings() {
    { let b = Box::new(String::from("first")); drop(*b); } { let b = Box::new(vec![1u8]); drop(b.len()); }
}

fn arms(cond: bool) {
    if cond { let b = Box::new(String::new()); drop(*b); } else { let b = Box::new(vec![1u8]); drop(b.len()); }
}
