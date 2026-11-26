// #1612: `let r;` assigned (and read) in some nested arms only. Its legs
// meet at both joins, which had no dot.
fn nested(s: String, t: String, a: bool, b: bool) {
    let r;
    if a {
        r = &s;
        println!("{}", r);
    } else {
        if b {
            r = &t;
            println!("{}", r);
        } else {
        }
    }
}

// The same in a `match`.
fn arms(s: String, t: String, n: u8) {
    let q;
    match n {
        0 => {
            q = &s;
            println!("{}", q);
        }
        1 => {
            q = &t;
            println!("{}", q);
        }
        _ => {}
    }
    println!("{}", s);
}

// #1615: two references copied into each other in one arm.
fn copied_both_ways(c: bool) {
    let s5 = String::from("a");
    let t5 = String::from("b");
    let mut a5 = &s5;
    let mut b5 = a5;
    if c {
        b5 = a5; a5 = b5; a5.len(); b5.len();
    } else {
        println!("{}", t5);
    }
    println!("{}", t5);
}

// In a loop the next pass may read w again (rustc's liveness doesn't order
// it by line there), so the join says only that the branches join.
fn in_loop(s: String, t: String, a: bool, b: bool) {
    for _i in 0..2 {
        let w;
        if a {
            w = &s;
            println!("{}", w);
        } else {
            if b {
                w = &t;
                println!("{}", w);
            } else {
            }
        }
    }
}

fn main() {
    nested(String::new(), String::new(), true, false);
    arms(String::new(), String::new(), 1);
    copied_both_ways(true);
    in_loop(String::new(), String::new(), false, true);
}
