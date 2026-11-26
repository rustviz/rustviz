fn run(mut t: (String, String, bool), c: bool) {
    match t {
        (ref a, _, true) | (_, ref a, false) => { t.0.push('x'); }
    }
}
