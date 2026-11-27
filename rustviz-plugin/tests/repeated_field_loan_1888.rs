struct Holder<'a> { r: &'a String }

fn repeated(t: String, s: String) {
    let mut h = Holder { r: &s };
    h.r = &t; h.r = &t; }
