// The note that says why a write to s8 compiles while r8's loan of it is
// drawn live ("On this path r8 isn't used again ...") took the write's
// own event id. Laying out the arm then treated it as the write, already
// placed, and left it on its source line: a row too high once rows were
// inserted above it, here by the earlier function's stacked arrows, so it
// was drawn above the inner fork as a dot of its own (#1443). The last
// function has the rows inserted above it in the same function.

fn else_if_ref_used_in_nested_else() {
    let mut o7 = Some(String::from("a"));
    let n = 1;
    if n > 2 {
        println!("{:?}", o7);
    } else if n > 0 {
        let r7 = o7.as_ref().unwrap();
        if n > 0 {
            let m7 = o7.as_mut().unwrap();
        } else {
            let w7 = r7;
            o7.take();
        }
        println!("{:?}", o7);
    }
}

fn else_if_shared_then_mut_in_nested_if() {
    let mut s8 = String::from("a");
    let n = 1;
    if n > 2 {
        println!("{}", s8);
    } else if n > 0 {
        let r8 = &s8;
        if n > 1 {
            let m8 = &mut s8;
        } else {
            println!("{}", r8);
        }
        println!("{}", s8);
    }
}

fn rows_inserted_in_same_fn() {
    let a = String::from("a");
    let b = String::from("b");
    let (p, q) = (a, b);
    let mut s9 = String::from("a");
    let n = 1;
    let r9 = &s9;
    if n > 1 {
        let m9 = &mut s9;
        m9.push('!');
    } else {
        println!("{}", r9);
    }
    println!("{} {} {}", s9, p, q);
}

fn main() {
    else_if_ref_used_in_nested_else();
    else_if_shared_then_mut_in_nested_if();
    rows_inserted_in_same_fn();
}
