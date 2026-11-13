fn main() {
    let mut s = String::from("hello");

    let r1 = &s;
    let r2 = &s;
    assert!(compare_strings(r1, r2));
    assert_eq!(r1, r2);
    assert_ne!(r1.len(), 0);
    debug_assert!(compare_strings(r1, r2));
    debug_assert_eq!(r1, r2);
    debug_assert_ne!(r2.len(), 0);

    let r3 = &mut s;
    clear_string(r3);
}

fn compare_strings(s1: &String, s2: &String) -> bool { // rustviz: hide
    *s1 == *s2
}

fn clear_string(s3: &mut String) { // rustviz: hide
    s3.clear();
}
