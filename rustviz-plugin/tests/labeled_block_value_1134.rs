// A labeled block's value (#1134): `break 'blk v` ends the block with v,
// otherwise its last expression is the value. Only one of them runs, so
// each moves (or copies, or lends) into the receiver only if it ended
// the block, as for a `loop` with several `break`s (#561).

fn take(_x: String) {}

fn bound(c: bool) {
    let a = String::from("a");
    let b = String::from("b");
    let x = 'blk: {
        if c {
            break 'blk a;
        }
        b
    };
    println!("{}", x);
}

fn assigned(c: bool) {
    let d = String::from("d");
    let e = String::from("e");
    let mut y = String::new();
    y = 'blk: {
        if c {
            break 'blk d;
        }
        e
    };
    println!("{}", y);
}

fn argument(c: bool) {
    let f = String::from("f");
    let g = String::from("g");
    take('blk: {
        if c {
            break 'blk f;
        }
        g
    });
}

fn borrowed(c: bool) {
    let h = String::from("h");
    let k = String::from("k");
    let r = 'blk: {
        if c {
            break 'blk &h;
        }
        &k
    };
    println!("{}", r);
}

fn copied(c: bool) {
    let m = 1;
    let n = 2;
    let z = 'blk: {
        if c {
            break 'blk m;
        }
        n
    };
    println!("{}", z);
}

fn new_value(c: bool) {
    let p = String::from("p");
    let w = 'blk: {
        if c {
            break 'blk p;
        }
        String::from("fresh")
    };
    println!("{}", w);
}

fn main() {
    bound(true);
    assigned(true);
    argument(true);
    borrowed(true);
    copied(true);
    new_value(true);
}
