// The macro's three match successors share a displayed destination.
macro_rules! match_value {
    ($r:ident) => {
        match *$r {
            1 => println!("one"),
            2 => println!("two"),
            _ => println!("other"),
        }
    };
}

fn number(n: u32) {
    let r = &n;
    match_value!(r);
}

fn main() {
    number(2);
}
