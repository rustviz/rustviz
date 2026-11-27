struct Guard<'a>(&'a mut String);

impl Guard<'_> {
    fn len(&self) -> usize { self.0.len() }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) { self.0.push('!'); }
}

fn run(mut s: String, t: String) {
    let x = Guard(
        &mut s,
    ).len();
    drop(t);
    let c = || { let _ = x; };
    s.push('?');
    c();
}
