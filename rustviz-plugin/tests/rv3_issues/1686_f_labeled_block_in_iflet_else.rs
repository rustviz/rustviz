fn main() {
    let mut v: Vec<String> = Vec::new();
    let s = String::new();
    if let Some(x) = v.first() {
    } else {
        'blk: {
            let r = &s;
            if v.len() > 1 { break 'blk; }
        }
    }
}
