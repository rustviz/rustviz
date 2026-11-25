// #1495: a loop inside an arm may give a moved-out variable a new value,
// or end before it does. After the loop the arm's line stops (as after a
// move), and the join says what s may hold.
fn moved_before(c: bool) {
    let mut s = String::from("a");
    drop(s);
    if c {
        for _i in 0..2 {
            s = String::from("b");
        }
    } else {
        println!("else");
    }
}

fn moved_in_arm(c: bool) {
    let mut t = String::from("a");
    if c {
        drop(t);
        for _i in 0..2 {
            t = String::from("b");
        }
    } else {
        println!("else");
    }
}

fn match_arm_loop(n: i32, c: bool) {
    let mut u = String::from("a");
    drop(u);
    match n {
        0 => {
            loop {
                if c { break; }
                u = String::from("b");
            }
        }
        _ => {
            println!("other");
        }
    }
}

fn while_else_if(c: bool, d: bool) {
    let mut w = String::from("a");
    if c {
        drop(w);
        while d {
            w = String::from("b");
        }
    } else if d {
        w = String::from("c");
    }
    println!("{}", d);
}

fn main() {
    moved_before(true);
    moved_in_arm(true);
    match_arm_loop(0, true);
    while_else_if(true, false);
}
