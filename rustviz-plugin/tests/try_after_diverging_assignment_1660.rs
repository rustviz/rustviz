fn run(c: bool, d: bool, e: bool, o: Option<u8>) -> Option<u8> {
    let mut s;
    if c {
        s = String::new();
    }
    if d {
        if e {
            s = String::new();
            return None;
        }
    }
    o?;
    Some(0)
}

fn in_argument(c: bool, flag: bool, value: Option<u8>) -> Option<u8> {
    let mut t;
    if c { t = String::new(); }
    if flag {
        if let Some(0) = value {
            t = String::new();
            return None;
        }
    }
    Some(std::hint::black_box(value?))
}

fn main() {}
