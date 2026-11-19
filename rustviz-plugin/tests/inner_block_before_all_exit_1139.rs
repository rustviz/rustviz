// A bare block that ends before a conditional whose every path leaves
// the function (#1139): the block's locals are dropped at its own `}`,
// which runs before the conditional.

fn if_else(c: bool) {
    {
        let a = String::from("a");
        println!("{}", a);
    }
    if c {
        return;
    } else {
        return;
    }
}

fn arms(n: i32) -> i32 {
    {
        let b = String::from("b");
        println!("{}", b);
    }
    match n {
        0 => return 1,
        _ => return 2,
    }
}

fn panics(c: bool, t: &str) {
    {
        {
            let d = String::from("d");
            println!("{}", d);
        }
        let e = String::from("e");
        println!("{}", e);
    }
    if c {
        return;
    } else {
        panic!("{}", t);
    }
}

fn siblings(c: bool) {
    {
        let f = String::from("f1");
        println!("{}", f);
    }
    {
        let f = String::from("f2");
        println!("{}", f);
    }
    if c {
        return;
    } else {
        return;
    }
}

fn exit_inside(c: bool) {
    let g = String::from("g");
    {
        let h = String::from("h");
        println!("{} {}", g, h);
        if c {
            return;
        } else {
            return;
        }
    }
}

fn main() {
    if_else(true);
    arms(1);
    panics(true, "x");
    siblings(true);
    exit_inside(true);
}
