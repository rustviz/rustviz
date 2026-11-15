// #927: a variable declared in a `match` arm whose name an earlier,
// out-of-scope variable used shares that variable's column. The column
// was drawn into the `match` as if a variable of that name existed
// there: "r is live in a conditional expression", "r has no value yet"
// and "r's borrow has ended on this path", where no `r` exists. The
// arm's `r` exists only in its arm, as if it had a name of its own.
fn main() {
    let n = 3;
    let a = String::from("a");
    {
        let r = &a;
        println!("{}", r);
    }
    match n {
        0 => {
            let r = &a;
            println!("{}", r);
        }
        _ => {}
    }
}

// The earlier `p` is declared in an `if` arm.
fn after_if(n: i32) {
    let a = String::from("a");
    if n > 1 {
        let p = &a;
        println!("{}", p);
    } else {
        println!("no");
    }
    match n {
        0 => {
            let p = &a;
            println!("{}", p);
        }
        _ => {}
    }
}

// Two owners.
fn owners(n: i32) {
    {
        let s = String::from("a");
        println!("{}", s);
    }
    match n {
        0 => {
            let s = String::from("b");
            println!("{}", s);
        }
        _ => {}
    }
}

// The later `w` is declared deeper in the arm, in a block in it. (Its
// scope end at the block's `}` is missing, #931.)
fn nested(n: i32) {
    let a = String::from("a");
    {
        let w = &a;
        println!("{}", w);
    }
    match n {
        0 => {
            {
                let w = &a;
                println!("{}", w);
            }
            println!("x");
        }
        _ => {}
    }
}
