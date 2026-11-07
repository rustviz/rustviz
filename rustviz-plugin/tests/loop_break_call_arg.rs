// A `loop` with several value `break`s as a call argument (#662). The
// call runs once the loop has ended, with whichever `break` ended it:
// the lenders stay borrowed until the call returns, on the loop's last
// line, and each pass to the call says which `break` it came from.
// rustc coerces each `break` value to the parameter's `&mut String`,
// which reborrows `ra` / `rb` instead of moving them.

fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let ra = &mut a;
    let rb = &mut b;
    let c = ra.len() > 0;
    take(loop {
        if c { break ra; } else { break rb; }
    });
    let n = 1;
}

fn take(s: &mut String) { s.push('!'); }

fn look(s: &String) { println!("{}", s); }

fn shared() {
    let e = String::from("e");
    let f = String::from("f");
    let re = &e;
    let rf = &f;
    let c = e.len() > 0;
    look(loop {
        if c { break re; } else { break rf; }
    });
    let n = 2;
}

fn one_break() {
    let mut g = String::from("g");
    let rg = &mut g;
    let c = rg.len() > 0;
    take(loop {
        if c { break rg; }
    });
    let n = 3;
}
