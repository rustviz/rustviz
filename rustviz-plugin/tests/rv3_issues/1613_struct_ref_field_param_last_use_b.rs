struct Counted<'a> {
    r: &'a String,
    n: i32,
}

fn in_loop_lent(m: &mut String) {
    loop {
        let p = &*m;
        let k = Counted { r: p, n: 1 };
        m.push('x');
        break;
    }
}
