// A caller can catch the panic and use the reference that crossed the call.
fn save<'a>(slot: &mut Option<&'a str>, value: &'a str) -> ! {
    *slot = Some(value);
    panic!();
}

fn forward<'a>(slot: &mut Option<&'a str>, value: &'a str) -> ! {
    save(slot, value);
}

fn nested(first: bool, second: bool) {
    let value = String::from("live after either break");
    let r = &value;
    loop {
        if first {
            if second { panic!("{}", r); }
            break;
        } else {
            if second { panic!("{}", r); }
            break;
        }
    }
    println!("{}", r);
}

fn payload(text: String) {
    let a = &text;
    let w = Some(a);
    if let Some(q) = w {
        panic!("{}", q);
    }
}

fn main() {
    let text = String::from("saved");
    let mut slot = None;
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| opaque(forward, &mut slot, &text)));
    assert_eq!(slot, Some("saved"));
}

fn opaque<'a>(f: fn(&mut Option<&'a str>, &'a str) -> !, slot: &mut Option<&'a str>, value: &'a str) -> ! {
    f(slot, value)
}
