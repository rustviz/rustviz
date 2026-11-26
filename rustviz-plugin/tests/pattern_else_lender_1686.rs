fn main() {
    let mut v = vec![String::new()];
    let s = String::new();
    if let Some(x) = v.first() {
        let r = v.last().unwrap();
        drop(s);
    } else {
        'blk: {
            let n = 1;
            v[0] = s;
            if v.len() > 1 { break 'blk; }
        }
    }
}
