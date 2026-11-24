// #1397: a temporary that a `let` extends through an `if` / `else`
// branch, a `match` arm, or a `Some(&..)` / `[&..]` / struct field in
// one (Rust 1.79+), is made only on the path through that branch. It
// gets a `(temporary)` column that starts in the branch, may or may not
// exist after the join (drawn like a `let x;` assigned in one branch),
// and is dropped by a drop flag at the end of the `let`'s block.
// (Checked with rustc: a plain assignment `r = if ..`, a call's argument
// and `loop { break &.. }` don't extend it, E0716.)
// `t` / `a` below are drawn borrowing the temporary (#1435: a holder
// bound from a branch value borrowed nothing, also for `Some(&s)`), and
// `early_return`'s `return` states the drop of the temporary it may hold
// (#1436, also for a `let x;` local). A one-line guarded arm
// (`Some(n) if n > 0 => &String::from("pos"),`) draws the temporary's
// value on the borrow's dot (#1441, `guard_arm_pairing_row_1441.rs`).
//
struct W<'a> {
    r: &'a String,
}

struct U {
    name: String,
}

fn make_u() -> U {
    U { name: String::from("u") }
}

fn one_line(c: bool) {
    let s = String::from("s");
    let r = if c { &String::from("x") } else { &s };
    println!("{}", r);
}

fn else_if(n: i32) {
    let s = String::from("s");
    let r = if n == 0 {
        &String::from("zero")
    } else if n == 1 {
        &s
    } else {
        &s
    };
    println!("{}", r);
}

fn arm(o: Option<i32>) {
    let s = String::from("s");
    let r = match o {
        Some(_) => &String::from("x"),
        None => &s,
    };
    println!("{}", r);
}

fn guard(o: Option<i32>) {
    let s = String::from("s");
    let r = match o {
        Some(n) if n > 0 => {
            &String::from("pos")
        }
        _ => &s,
    };
    println!("{}", r);
}

fn some_in_branch(c: bool) {
    let t = if c {
        Some(&String::from("x"))
    } else {
        None
    };
    println!("{:?}", t);
}

fn array_in_branch(c: bool) {
    let s = String::from("s");
    let a = if c {
        [&String::from("x")]
    } else {
        [&s]
    };
    println!("{:?}", a);
}

fn field_in_branch(c: bool) {
    let s = String::from("s");
    let w = if c {
        W { r: &String::from("x") }
    } else {
        W { r: &s }
    };
    println!("{}", w.r);
}

fn nested(c: bool, d: bool) {
    let s = String::from("s");
    let r = if c {
        if d {
            &String::from("inner")
        } else {
            &s
        }
    } else {
        &s
    };
    println!("{}", r);
}

fn both(c: bool) {
    let r = if c {
        &String::from("a")
    } else {
        &String::from("b")
    };
    println!("{}", r);
}

fn block_in_branch(c: bool) {
    let s = String::from("s");
    let r = if c {
        let n = 1;
        println!("{}", n);
        &String::from("x")
    } else {
        &s
    };
    println!("{}", r);
}

fn if_let(o: Option<i32>) {
    let s = String::from("s");
    let r = if let Some(_) = o {
        &String::from("x")
    } else {
        &s
    };
    println!("{}", r);
}

fn tuple_elem(c: bool) {
    let s = String::from("s");
    let t = (if c { &String::from("x") } else { &s }, 1);
    println!("{:?}", t);
}

fn ctor_arg(c: bool) {
    let s = String::from("s");
    let o = Some(if c { &String::from("x") } else { &s });
    println!("{:?}", o);
}

fn temp_field(c: bool) {
    let s = String::from("s");
    let r = if c {
        &make_u().name
    } else {
        &s
    };
    println!("{}", r);
}

fn mut_temp(c: bool) {
    let mut s = String::from("s");
    let r = if c {
        &mut String::new()
    } else {
        &mut s
    };
    r.push('x');
    println!("{}", r);
}

fn inner_block(c: bool) {
    let s = String::from("s");
    {
        let r = if c {
            &String::from("x")
        } else {
            &s
        };
        println!("{}", r);
    }
    println!("{}", s);
}

fn early_return(c: bool, d: bool) {
    let s = String::from("s");
    let r = if c {
        &String::from("x")
    } else {
        &s
    };
    if d {
        return;
    }
    println!("{}", r);
}

fn as_str(c: bool) {
    let r: &str = if c {
        &String::from("x")
    } else {
        "lit"
    };
    println!("{}", r);
}

fn in_loop(n: i32) {
    let s = String::from("s");
    for i in 0..n {
        let r = if i == 0 {
            &String::from("x")
        } else {
            &s
        };
        println!("{}", r);
    }
}

// #1437: a `let x;` assigned in an inner branch is maybe-initialized
// after the outer join too (that join said "none of the branches above
// assigned it").
fn letx_nested(c: bool, d: bool) {
    let s;
    if c {
        if d {
            s = String::from("x");
            println!("{}", s);
        } else {
            println!("no");
        }
    } else {
        println!("no");
    }
    println!("end");
}

fn main() {
    one_line(true);
    else_if(0);
    arm(Some(1));
    guard(Some(1));
    some_in_branch(true);
    array_in_branch(true);
    field_in_branch(true);
    nested(true, true);
    both(true);
    block_in_branch(true);
    if_let(Some(1));
    tuple_elem(true);
    ctor_arg(true);
    temp_field(true);
    mut_temp(true);
    inner_block(true);
    early_return(true, false);
    as_str(true);
    in_loop(2);
    letx_nested(true, true);
}
