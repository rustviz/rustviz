fn with_else() {
    let y = Some(String::from("new"));
    let mut z = String::from("old");
    drop(z);
    if let Some(a) = y && { z = a; z.len() > 5 } {
        println!("{}", z);
    } else {
        println!("short");
    }
}

fn without_else() {
    let y = Some(String::from("new"));
    let mut w = String::from("old");
    drop(w);
    if let Some(a) = y && { w = a; w.len() > 5 } {
        println!("{}", w);
    }
    w = String::from("again");
    println!("{}", w);
}

fn main() {
    with_else();
    without_else();
}
