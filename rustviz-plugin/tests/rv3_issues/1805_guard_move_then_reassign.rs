pub fn f(k: u8) {
    let mut s = String::from("a");
    match k {
        0 | 0..=1 if { drop(s); s = String::new(); false } => {}
        _ => println!("{s}"),
    }
}
