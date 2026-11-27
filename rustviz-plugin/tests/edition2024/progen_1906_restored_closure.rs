fn run(s: String, restore: bool, consume: bool) {
    let mut c = move || drop(s);
    if restore {
        let saved = c;
        c = saved;
    }
    if consume {
        c();
    }
}

fn conditional(s: String, init: bool) {
    let c;
    if init {
        c = move || drop(s);
    }
}
