fn first<'a>(s: &'a String) -> &'a str {
    s.as_str()
}

fn main() {
    let mut s = String::from("hi");
    let o = Some(s);
    let mut v: Vec<String> = Vec::new();
    match v.as_slice() {
        [] => {
        }
        [x2] => {
        }
        [x21, rest @ ..] => {
            if let Some(x3) = v.first() && x21.is_empty() && let Some(y) = v.last() {
                s = String::from("rust");
            }
        }
    }
    'blk2: {
        if v.len() > 1 { break 'blk2; }
        s = String::from("rust");
        let c = move || s;
    }
    let m3 = v.last_mut().unwrap();
}
