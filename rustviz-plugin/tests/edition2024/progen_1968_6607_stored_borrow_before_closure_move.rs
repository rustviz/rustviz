fn main() {
    let s = String::from("hey");
    let vr = vec![s.as_str(), "abc"];
    if vr.is_empty() {
        let _ = 'blk: {
            if vr.is_empty() { break 'blk String::from("x"); }
            String::from("x")
        };
    } else {
        'blk2: {
            let c = move || s;
            if true {
                return;
            }
            if true { break 'blk2; }
        }
    }
}
