// #1504: a labeled block (or `loop`) every way out of which moves s: the
// `break` that carries s, and the code on the way to the block's end.
// s is moved after the block on every path; no `break` comes "before s is
// moved".
fn id(t: String) -> String { t }

fn first(flag: bool) {
    let s = String::from("a");
    let t = 'blk: {
        if flag { break 'blk s; }
        id(s)
    };
}

fn second(flag: bool) {
    let s2 = String::from("a");
    let t2 = 'blk: {
        if flag { break 'blk s2; }
        drop(s2);
        String::new()
    };
}

fn third(flag: bool) {
    let s3 = String::from("a");
    let t3 = loop {
        if flag { break s3; }
        break id(s3);
    };
}

fn fourth(c: u8) {
    let s4 = String::from("a");
    let t4 = 'blk: {
        if c == 0 { break 'blk s4; }
        if c == 1 { drop(s4); break 'blk String::new(); }
        drop(s4);
        String::from("z")
    };
}

fn main() {
    first(true);
    second(true);
    third(true);
    fourth(0);
}
