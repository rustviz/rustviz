fn consume(n: usize) -> usize { n }

pub fn run() -> usize {
    let count = || {
        let local = String::from("closure");
        drop(local);
        1
    };
    consume({
        let arg = String::from("argument");
        let len = arg.len();
        drop(arg);
        len + count()
    })
}

pub fn stopped() {
    let s = String::new();
    return;
    drop(s);
}
