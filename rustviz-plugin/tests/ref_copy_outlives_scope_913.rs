// #913: a reference copied into a variable that outlives it shares its
// loan, which lasts until the copy's last use (rustc rejects a
// `t.push('!')` before the `println!`). The copy gives t back there; w's
// own end is at its last use, the copy (Q7).
fn copied_out() {
    let a = String::from("a");
    let t = String::from("t");
    let mut x = &a;
    {
        let w = &t;
        x = w;
    }
    println!("{}", x);
}

// Copied through a `&mut` to x.
fn through_mut_ref() {
    let a2 = String::from("a2");
    let t2 = String::from("t2");
    let mut x2 = &a2;
    {
        let w2 = &t2;
        let r2 = &mut x2;
        *r2 = w2;
    }
    println!("{}", x2);
}

// A loop variable copied out on each iteration.
fn in_loop() {
    let a3 = String::from("a3");
    let words = vec![String::from("w")];
    let mut x3 = &a3;
    for w3 in words.iter() {
        let r3 = &mut x3;
        *r3 = w3;
    }
    println!("{}", x3);
}

// Stored in a Vec that outlives it.
fn into_vec() {
    let t4 = String::from("t4");
    let mut v4: Vec<&String> = Vec::new();
    {
        let w4 = &t4;
        v4.push(w4);
    }
    println!("{}", v4.len());
}

fn main() {
    copied_out();
    through_mut_ref();
    in_loop();
    into_vec();
}
