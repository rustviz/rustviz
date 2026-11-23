// #1296: a conditional in a `loop` whose arms all leave (`break`,
// `continue`, a `break` carrying a value drawn at the loop's end, #431):
// nothing reaches its end, but the code after the loop is reached through
// the `break`s (#759), so the columns go on. They go on from a dot where
// the conditional ends, which says so; they used to start there in
// mid-air, below the arms' stop bars.

fn break_or_continue() -> String {
    let mut n = 0;
    let q = loop {
        n += 1;
        let j = String::from("j");
        if n == 6 {
            break j;
        } else {
            continue;
        }
    };
    q
}

fn break_continue_match() -> String {
    let mut n = 0;
    loop {
        n += 1;
        let j = String::from("j");
        match n {
            3 => break j,
            _ => continue,
        }
    }
}

fn while_both_break(c: bool) {
    let a = String::from("a");
    let ra = &a;
    while true {
        if c { println!("{}", ra); break; } else { break; }
    }
    println!("{}", a);
}

fn nested_break(c: bool, d: bool) {
    let s = String::from("s");
    let t = loop {
        if c {
            if d { break s; } else { break String::new(); }
        } else {
            continue;
        }
    };
    println!("{}", t);
}

fn main() {
    break_or_continue();
    break_continue_match();
    while_both_break(true);
    nested_break(true, true);
}
