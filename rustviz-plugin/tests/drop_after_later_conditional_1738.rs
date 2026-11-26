fn no_move(c: bool, d: bool, stop: bool) {
    let mut z;
    if c {
        z = String::new();
    }
    if d {
        z = String::new();
    }
    if stop { return; }
    z = String::from("last");
    println!("{}", z);
}

fn with_try(c: bool, d: bool, o: Option<()>) -> Option<()> {
    let mut x;
    if c {
        x = String::new();
    }
    if d {
        x = String::new();
    }
    o?;
    x = String::from("last");
    println!("{}", x);
    Some(())
}

fn main() {}
