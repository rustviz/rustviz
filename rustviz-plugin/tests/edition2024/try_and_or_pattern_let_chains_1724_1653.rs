fn try_in_let_chain(c: bool, a: Option<u8>, b: Option<u8>) -> Option<()> {
    let mut chained = String::from("old");
    if let 0 = a? && c { chained = String::new(); } b?;
    println!("{chained}");
    Some(())
}

fn after_let_chain(enabled: bool) {
    let chain_pair = (String::from("left"), String::from("right"), false);
    if let (x, _, false) | (_, x, false) = chain_pair && enabled {
        println!("{x}");
    }
}
