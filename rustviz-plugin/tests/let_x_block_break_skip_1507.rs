// #1507: a `let s;` local given its value in each branch, then moved in a
// labeled block (or loop) after a `break` that skips the move. If the
// `break` runs, s still holds its String, dropped where s goes out of
// scope; otherwise it was moved. A drop flag decides at runtime.
fn first(flag: bool) {
    let s;
    if flag {
        s = String::from("a");
    } else {
        s = String::from("b");
    }
    'blk: {
        if s.len() > 1 { break 'blk; }
        drop(s);
    }
}

fn second(flag: bool) {
    let s2;
    if flag {
        s2 = String::from("a");
    } else {
        s2 = String::from("b");
    }
    'blk: {
        if s2.len() > 1 { break 'blk; }
        let c = move || s2;
    }
}

fn third(flag: bool) {
    let s3;
    if flag {
        s3 = String::from("a");
    } else {
        s3 = String::from("b");
    }
    loop {
        if s3.len() > 1 { break; }
        drop(s3);
        break;
    }
}

fn main() {
    first(true);
    second(true);
    third(true);
}
