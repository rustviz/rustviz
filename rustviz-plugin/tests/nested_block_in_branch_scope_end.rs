// A bare block nested in a branch ends its own locals at its `}`
// (#591): the String is dropped there, not left without an end.

fn in_if_arm(c: bool) {
    if c {
        {
            let u = String::from("a");
            println!("{}", u);
        }
        println!("after");
    }
}

fn in_else_arm(c: bool) {
    if c {
        println!("then");
    } else {
        {
            let w = String::from("b");
            println!("{}", w);
        }
    }
}

fn in_match_arm(n: i32) {
    match n {
        0 => {
            {
                let m = String::from("c");
                println!("{}", m);
            }
        }
        _ => println!("other"),
    }
}

fn in_loop(c: bool) {
    for _i in 0..2 {
        if c {
            {
                let k = String::from("d");
                println!("{}", k);
            }
        }
    }
}

fn main() {
    in_if_arm(true);
    in_else_arm(false);
    in_match_arm(0);
    in_loop(true);
}
