fn run(c: bool, d: bool, e: bool, v: Vec<u8>) -> Option<u8> {
    let mut bt = Box::new(String::from("ab"));
    if c {
        let _s = *bt;
    }
    if d {
        bt = Box::new(String::from("cd"));
    }
    if e {
    } else {
        let _n = v.last()?;
    }
    Some(0)
}
