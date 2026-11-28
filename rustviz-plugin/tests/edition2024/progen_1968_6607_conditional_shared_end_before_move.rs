fn first(s: &String) -> &str {
    s.as_str()
}

fn main() {
    let s = String::from("ab");
    let x = first(&s);
    let v = vec![String::from("hi")];
    'blk: {
        if x.len() > 1 {
            return;
        }
        if let Some(x2) = v.first() { println!("{}", x); } else { let c = move || s; }
        if v.len() > 1 { break 'blk; }
    }
}
