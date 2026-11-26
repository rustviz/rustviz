fn f7(c: bool) {
    let mut s = String::from("a");
    if c {
        let r = &s; let n = r.len(); let m = &mut s;
        m.push('b');
    } else {
        println!("{}", s);
    }
}
