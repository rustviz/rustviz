// A struct's reference field re-pointed in one arm and read in the other:
// on the reading path h.r still points to s, and its borrow ends at that
// read, in that arm; on the re-pointing path the old loan ends at the
// write. h.r's join says its borrow ended in each branch (#1576). s used
// to be given back between the arms' legs, and h.r's legs met with no dot.

struct Holder<'a> {
    r: &'a String,
}

fn same_lender() {
    let s = String::from("hey");
    let mut h = Holder { r: &s };
    if s.len() > 1 {
        h.r = &s;
    } else {
        println!("{}", h.r);
    }
}

fn in_a_loop() {
    let s = String::from("hey");
    let t = String::from("t");
    let mut h = Holder { r: &s };
    loop {
        if s.len() > 1 {
            h.r = &t;
        } else {
            println!("{}", h.r);
        }
        break;
    }
}

fn in_a_match(s: String, t: String, n: i32) {
    let mut h = Holder { r: &s };
    match n {
        0 => {
            h.r = &t;
        }
        _ => {
            println!("{}", h.r);
        }
    }
}

fn main() {}
