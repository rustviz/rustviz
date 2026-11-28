fn run(mut o: (Option<String>, i32), c: bool, d: bool) {
    if c { let (Some(s), _) = o else { return }; drop(s); }
    if d { o = (None, 0); } else { o = (None, 1); }
}
