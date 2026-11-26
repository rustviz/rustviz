pub fn g(n: u8, mut o: Option<u8>) -> Option<u8> {
    let mut lt = String::new();
    drop(lt);
    let mut i = 0;
    loop {
        o?;
        lt = String::new();
        i += 1; o = None;
        if i > n {
            return Some(0);
        }
    }
}
