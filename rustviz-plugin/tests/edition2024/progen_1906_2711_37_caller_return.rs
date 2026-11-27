fn read(_: &String) {}
fn run(m: &mut String, a: bool, b: bool) {
    let r = &*m;
    if a {
        if b {
            m.push('x');
        } else if a {
            let other = &mut *m;
        } else {
            read(r);
        }
    } else {
        let next = &*m;
    }
}
