fn run(branch: bool, store: bool, mut v: Vec<String>) {
    if branch {
        for e in v.iter_mut() {
            if store {
                let child = &mut *e;
                child.push('a');
                child.push('b');
            } else {
                e.push('c');
            }
        }
        v.clear();
    } else {
        v.clear();
    }
}
