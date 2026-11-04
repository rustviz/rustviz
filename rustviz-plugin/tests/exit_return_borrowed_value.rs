// `return s.len()` only borrows `s`, so `s` is dropped at the
// `return` (#410), with `t`. `return format!("{}!", u)` borrows `u`
// too. `return v` moves `v` to the caller: it isn't dropped there.

fn first_len(s: String, short: bool) -> usize {
    let t = String::from("t");
    if short {
        return s.len();
    } else {
        drop(t.clone());
    }
    s.len() + t.len()
}

fn shout(u: String, v: String, which: i32) -> String {
    match which {
        0 => return format!("{}!", u),
        1 => return v,
        _ => {}
    }
    u + &v
}

fn main() {
    println!("{}", first_len(String::from("s"), true));
    println!("{}", shout(String::from("u"), String::from("v"), 0));
}
