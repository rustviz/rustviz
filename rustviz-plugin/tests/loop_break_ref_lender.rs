// A reference carried out of a `loop` by `break` (#585): the receiver
// borrows what the reference borrows, as `let r = ra;` would. A `&mut`
// moves on to `r`, so `a` stays borrowed until `r`'s last use, and `a`
// still owns its String at the end of `main`.

fn main() {
    let mut a = String::from("a");
    let ra = &mut a;
    let r = loop {
        break ra;
    };
    r.push('!');
    println!("{}", r);
}

fn shared() {
    let b = String::from("b");
    let rb = loop {
        break &b;
    };
    println!("{}", rb);
}

fn in_branch() {
    let mut c = String::from("c");
    let rc = &mut c;
    let mut i = 0;
    let r = loop {
        i += 1;
        if i == 3 { break rc; }
    };
    r.push('c');
    println!("in branch: {}", r);
}

fn in_loop() {
    let mut d = String::from("d");
    let mut i = 0;
    let r = loop {
        i += 1;
        let k = &mut d;
        if i == 3 { break k; }
    };
    r.push('d');
    println!("in loop: {}", r);
}

fn several() {
    let e = String::from("e");
    let f = String::from("f");
    let c = e.len() > 0;
    let r = loop {
        if c { break &e; } else { break &f; }
    };
    println!("several: {}", r);
    let g = e;
    println!("{}", g);
}
