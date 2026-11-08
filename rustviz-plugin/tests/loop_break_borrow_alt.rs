// Several value `break`s that borrow (#657, #661): only the `break` that
// ended the loop made its borrow, so each borrow says which `break` it
// came from. rustc coerces `break rb` to the type `break ra` fixed, which
// reborrows `rb` (`&mut *rb`) instead of moving it.

fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let ra = &mut a;
    let rb = &mut b;
    let c = ra.len() > 0;
    let r = loop {
        if c { break ra; } else { break rb; }
    };
    r.push('!');
    println!("{}", r);
}

fn shared() {
    let e = String::from("e");
    let f = String::from("f");
    let c = e.len() > 0;
    let r = loop {
        if c { break &e; } else { break &f; }
    };
    println!("{}", r);
}

fn fresh_mut() {
    let mut g = String::from("g");
    let mut h = String::from("h");
    let c = g.len() > 0;
    let r = loop {
        if c { break &mut g; } else { break &mut h; }
    };
    r.push('!');
}

fn assign() {
    let mut p = String::from("p");
    let mut q = String::from("q");
    let mut z = String::from("z");
    let rp = &mut p;
    let rq = &mut q;
    let c = rp.len() > 0;
    let mut r = &mut z;
    r.push('?');
    r = loop {
        if c { break rp; } else { break rq; }
    };
    r.push('!');
}
