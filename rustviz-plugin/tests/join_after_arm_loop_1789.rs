fn loop_in_arm_then_assign(c: bool, items: Vec<u8>) {
    let mut s;
    if c {
        for _ in items {
            s = String::new();
        }
    }
    s = String::from("last");
    println!("{}", s);
}

fn match_arm(tag: Option<usize>) {
    let mut t;
    match tag {
        Some(n) => {
            for _ in 0..n { t = String::new(); }
        }
        None => {}
    }
    t = String::from("last");
}

fn main() {}
