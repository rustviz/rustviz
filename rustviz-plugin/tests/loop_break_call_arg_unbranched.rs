// A `loop` with a single, unbranched `break` as a call argument,
// spread over several lines (#677). The call runs once the loop has
// ended, on the loop's last line (`});`); the borrow it uses ends
// there too. The call's dot must sit on that same row, not up on the
// `break` row (the branched form is `loop_break_call_arg`, #662).
// rustc coerces the `break` value to the parameter's `&mut String`,
// which reborrows `r` instead of moving it.

fn take(s: &mut String) { s.push('!'); }

fn look(s: &String) { println!("{}", s); }

fn main() {
    let mut a = String::from("a");
    let r = &mut a;
    take(loop {
        break r;
    });
    let n = 1;
}

fn shared() {
    let e = String::from("e");
    let re = &e;
    look(loop {
        break re;
    });
    let n = 2;
}
