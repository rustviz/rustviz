struct Counted<'a> {
    r: &'a String,
    n: i32,
}

fn plain(p: &String) {
    let k = Counted { r: p, n: 1 };   // line 7: k's last use
    let z = 5;
}                                     // line 9: "k.r's immutable borrow ends / k.r goes out of scope"

fn in_loop(p: &String) {
    loop {
        let k = Counted { r: p, n: 1 };
        break;
    }                                 // "k.r's immutable borrow ends" here, two rows after k's last use
}
