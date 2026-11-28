struct Marker;
impl Drop for Marker {
    fn drop(&mut self) {
        println!("drop marker");
    }
}

fn guard(candidate: Result<u8, u8>) -> u8 {
    let f = || true;
    match candidate {
        Ok(n) | Err(n) if f() => n,
        _ => 0,
    }
}

fn staged_borrow(arg: Option<u32>) -> Option<()> {
    let mut sum = 0;
    let mut f = |n| { sum += n; };
    f(arg?);
    Some(())
}

fn staged_move(take: bool, bail: bool) {
    let captured = Marker;
    let f = move |()| { println!("called"); drop(captured); };
    let _: Option<()> = 'out: {
        if take {
            f(if bail { break 'out None } else { () });
        }
        Some(())
    };
    println!("scope end");
}

fn mutate_then_return() -> String {
    let mut text = String::from("text");
    let f = move || { text.push('!'); text };
    f()
}

fn return_or_loop(wait: bool) -> String {
    let held = String::from("held");
    let f = move || {
        if wait { loop {} }
        held
    };
    f()
}

struct Token(u32);

fn token_value(token: &Token) -> u32 {
    token.0
}

fn finish_with_token() {
    let token = Token(7);
    let captured = Marker;
    let f = move || {
        println!("{}", token_value(&token));
        drop(captured);
    };
    f();
}

fn onward(value: String) {
    println!("{value}");
}

fn return_or_transfer(return_it: bool) -> String {
    let held = String::from("held");
    let f = move || {
        if return_it { held } else { onward(held); String::from("fresh") }
    };
    f()
}

fn read_pair<'a>(mut p: (&'a str, &'a str), write: impl FnOnce(&mut &'a str)) -> (&'a str, &'a str) {
    write(&mut p.0);
    p
}

fn read_raw_slot<'a>(slot: *mut &'a str, write: impl FnOnce(*mut &'a str)) -> &'a str {
    write(slot);
    unsafe { *slot }
}

fn main() {
    assert_eq!(guard(Err(7)), 7);
    assert_eq!(staged_borrow(None), None);
    staged_move(true, true);
    staged_move(true, false);
    staged_move(false, false);
    assert_eq!(mutate_then_return(), "text!");
    assert_eq!(return_or_loop(false), "held");
    finish_with_token();
    assert_eq!(return_or_transfer(false), "fresh");
    assert_eq!(read_pair(("old", "right"), |field| *field = "new"), ("new", "right"));
    let mut slot = "old";
    assert_eq!(read_raw_slot(&mut slot, |ptr| unsafe { *ptr = "new" }), "new");
}
