pub fn check(pattern: bool, initialized: bool, leave: bool) {
    let guarded = String::new();
    match pattern {
        true if { drop(guarded); false } => {}
        _ => {
            std::hint::black_box(());
        }
    }
    let value: String;
    if leave {
        value = String::new();
        drop(value);
        return;
    }
    if initialized { value = String::new(); }
    std::hint::black_box(());
}
