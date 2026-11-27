fn maybe(s: String) -> Option<String> {
    Some(s)
}

fn main() {
    let s = String::from("x");
    let mut s3 = if s.len() > 1 {
        String::from("hey")
    } else {
        String::from("ab")
    };
    let c = move || s3;
    let Some(x) = maybe(String::from("x")) else {
        return;
    };
    let mut r3 = &x;
    let s4 = 'blk: {
        if r3.is_empty() { break 'blk c(); }
        r3.clone()
    };
}
