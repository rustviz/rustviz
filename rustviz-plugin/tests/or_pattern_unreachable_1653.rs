fn after_let() {
    let mut u = (String::from("u0"), String::from("u1"));
    let ((y, _) | (_, y)) = u;
    println!("{}", y);
    u = (String::from("u2"), String::from("u3"));
    println!("{}", u.0);
}

fn after_match() {
    let mut v = (String::from("v0"), String::from("v1"), false);
    match v {
        (x, _, false) | (_, x, false) => println!("{x}"),
        _ => {}
    }
    v = (String::new(), String::new(), true);
    println!("{}", v.2);
}

fn before_scope_end() {
    let w = (String::from("w0"), String::from("w1"), String::from("w2"));
    let ((x, _, _) | (_, x, _)) = w;
    println!("{x} {}", w.2);
}

fn after_if_let() {
    let if_pair = (String::from("left"), String::from("right"), false);
    if let (x, _, false) | (_, x, false) = if_pair {
        println!("{x}");
    }
}

fn after_let_else() {
    let else_pair = (String::from("left"), String::from("right"), false);
    let ((x, _, false) | (_, x, false)) = else_pair else { return };
    println!("{x}");
}

fn before_try(keep_going: Option<()>) -> Option<()> {
    let pending = (String::from("left"), String::from("right"), String::from("untouched"));
    let ((x, _, _) | (_, x, _)) = pending;
    keep_going?;
    println!("{x} {}", pending.2);
    Some(())
}

fn in_loop() {
    let mut loop_pair = (String::from("left"), String::from("right"), false);
    while let (x, _, false) | (_, x, false) = loop_pair {
        println!("{x}");
        loop_pair = (String::new(), String::new(), true);
    }
    println!("{}", loop_pair.2);
}
