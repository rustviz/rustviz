struct Holder<'a> { view: &'a String }

fn sibling(flag: bool, a: &mut String, b: &mut String) {
    let mut r = &mut *a;
    let m = &mut *r;
    if flag {
        r = &mut *b;
        r.clear();
    } else {
        let h = Holder { view: &*m }; println!("{}", h.view);
    }
}
