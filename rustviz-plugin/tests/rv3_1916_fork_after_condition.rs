fn check(take: bool, c: bool) {
    let mut s = String::new();
    if { if take { drop(s); } c } {
        s = String::new();
        println!("{s}");
    }
}
