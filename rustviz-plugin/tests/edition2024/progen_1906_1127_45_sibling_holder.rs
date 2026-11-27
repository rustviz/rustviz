fn run(mut o: Option<String>, flag: bool) {
    if let Some(x) = &mut o {
        x.push('a');
    } else {
        match o.as_ref() {
            Some(y) => { println!("{y}"); }
            None => { let next = o.as_mut(); }
        }
    }
}
