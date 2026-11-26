fn while_chain() {
    let mut o = Some(String::from("rust"));
    while let Some(x) = &o && x.len() > 1 { o = None; }
}
