fn main() {
    let s = String::from("a");
    let mut s2 = s;
    let c = s2.len() > 3;
    let t = s2;
    'blk: {
        if c { break 'blk; }
        s2 = String::from("b");
    }
    println!("{}", t);
}

fn destructuring(c: bool) {
    let mut pair_a = String::from("a");
    let mut pair_b = String::from("b");
    let old = (pair_a, pair_b);
    'block: {
        if c { break 'block; }
        (pair_a, pair_b) = (String::from("new a"), String::from("new b"));
    }
    drop(old);
}

fn both_paths_break(c: bool) {
    let mut either = String::from("a");
    let both_old = either;
    'block: {
        if c {
            either = String::from("b");
            break 'block;
        }
        break 'block;
    }
    drop(both_old);
}

fn reassign_after(c: bool) {
    let mut later = String::from("a");
    let old = later;
    'block: {
        if c { break 'block; }
        later = String::from("b");
    }
    later = String::from("c");
    println!("{later}");
    drop(old);
}

fn try_after(c: bool, r: Result<(), ()>) -> Result<(), ()> {
    let mut pending = String::from("a");
    let old = pending;
    'block: {
        if c { break 'block; }
        pending = String::from("b");
    }
    r?;
    drop(old);
    Ok(())
}

// Both ways out assign: the `break`'s value does it on its way out.
fn break_value_assigns(c: bool) {
    let operand_s;
    'b: {
        if c {
            break 'b { operand_s = String::new(); };
        }
        operand_s = String::new();
    }
    drop(operand_s);
}

// The fallthrough never finishes: only the assigning `break` gets out.
fn fallthrough_returns(c: bool) {
    let return_s;
    'b: {
        if c {
            return_s = String::new();
            break 'b;
        }
        let _ = return;
    }
    drop(return_s);
}

// An immutable `let`: it can't be assigned again (E0384).
fn immutable_deferred(c: bool) {
    let immut_s: String;
    'b: {
        if c { break 'b; }
        immut_s = String::from("x");
        println!("{immut_s}");
    }
}

// The break's value only reads read_s: read_s still holds its string.
fn break_value_reads(c: bool) {
    let mut read_s = String::from("a");
    'b: {
        if c {
            break 'b { let _ = read_s.len(); };
        }
        read_s = String::new();
    }
    drop(read_s);
}

// The way to the block's end stops inside a call's argument, or in a
// nested block: only the assigning `break` gets out.
fn fallthrough_returns_in_argument(c: bool) {
    let arg_s;
    'b: {
        if c {
            arg_s = String::new();
            break 'b;
        }
        let _ = Some(return);
    }
    drop(arg_s);
}

fn fallthrough_returns_in_block(c: bool) {
    let nested_s;
    'b: {
        if c {
            nested_s = String::new();
            break 'b;
        }
        {
            let _ = return;
        }
    }
    drop(nested_s);
}

// `break 'b ()` skips the assignment just like `break 'b`.
fn unit_break_value(c: bool) {
    let mut unit_s = String::from("a");
    let unit_old = unit_s;
    'b: {
        if c {
            break 'b ();
        }
        unit_s = String::from("b");
    }
    unit_s = String::from("c");
    println!("{unit_s} {unit_old}");
}

// A field moved before the block and assigned on its way to the end:
// the field join stays as on main.
fn field_assigned(c: bool) {
    let mut p = (String::from("a"), String::from("b"));
    drop(p.0);
    'b: {
        if c {
            break 'b;
        }
        p.0 = String::new();
    }
    println!("{}", p.1);
}
