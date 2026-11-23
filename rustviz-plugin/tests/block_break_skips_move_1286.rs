// #1286: a `break 'blk` that comes before a move skips it, so after the
// block the variable may still hold its value: the block's end says
// why, and the scope end's drop flag has its reason. The same for a
// valueless `break 'blk`, a `break` in a match arm, a move in the
// `else` branch, a field, a labeled block inside a loop, and a
// `break 'outer` out of a nested block. When every way out of the block
// (its `break`s and its end) comes after the move, it was moved.
struct P { name: String, bio: String }

fn demo(c: bool) {
    let s = String::from("a");
    let x = 'blk: {
        if c {
            break 'blk 1;
        }
        drop(s);
        2
    };
    println!("{}", x);
}

fn unit(c: bool) {
    let u = String::from("a");
    'blk: {
        if c {
            break 'blk;
        }
        drop(u);
    }
}

fn in_arm(n: i32) -> i32 {
    let a = String::from("a");
    let x = 'blk: {
        match n {
            0 => break 'blk 1,
            _ => {}
        }
        let t = a;
        2
    };
    x
}

fn else_branch(c: bool) -> i32 {
    let e = String::from("a");
    let x = 'blk: {
        if c {
            break 'blk 1;
        } else {
            drop(e);
        }
        2
    };
    x
}

fn field(c: bool) -> i32 {
    let p = P { name: String::from("a"), bio: String::from("b") };
    let x = 'blk: {
        if c {
            break 'blk 1;
        }
        drop(p.name);
        2
    };
    x
}

fn in_loop(c: bool) {
    let w = String::from("a");
    loop {
        let x = 'blk: {
            if c {
                break 'blk 1;
            }
            drop(w);
            2
        };
        println!("{}", x);
        break;
    }
}

fn nested(c: bool, d: bool) -> i32 {
    let n = String::from("a");
    let x = 'outer: {
        let y = 'inner: {
            if c {
                break 'outer 1;
            }
            if d {
                break 'inner 3;
            }
            4
        };
        drop(n);
        y
    };
    x
}

fn both(c: bool) -> i32 {
    let b = String::from("a");
    let x = 'blk: {
        if c {
            drop(b);
            break 'blk 1;
        }
        drop(b);
        2
    };
    x
}

fn main() {
    demo(true);
    unit(true);
    let _ = in_arm(0);
    let _ = else_branch(true);
    let _ = field(true);
    in_loop(true);
    let _ = nested(true, false);
    let _ = both(true);
}
