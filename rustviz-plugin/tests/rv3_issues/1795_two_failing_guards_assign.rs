fn f(k: u8) {
    let mut z = String::from("orig");
    match k {
        0 if { z = String::from("first"); false } => {}
        0 if { z = String::from("second"); false } => {}
        _ => println!("{z}"),
    }
}
