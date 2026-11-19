// #1226: a `let` pattern that takes apart a labeled block's value gets
// whichever value ended the block: a `break 'blk`'s, or the block's last
// expression. s moves into x only on the `break` path, t only on the
// other; each is drawn as one of them, where the block ends (as for
// `let x = 'blk: { … }`, #1134).
struct P {
    a: String,
    b: i32,
}

fn tuple(c: bool) {
    let s = String::from("a");
    let t = String::from("b");
    let (x, n) = 'blk: {
        if c {
            break 'blk (s, 1);
        }
        (t, 2)
    };
    println!("{} {}", x, n);
}

fn strukt(c: bool) {
    let u = String::from("a");
    let v = String::from("b");
    let P { a, b } = 'blk: {
        if c {
            break 'blk P { a: u, b: 1 };
        }
        P { a: v, b: 2 }
    };
    println!("{} {}", a, b);
}

fn main() {
    tuple(true);
    strukt(false);
}
