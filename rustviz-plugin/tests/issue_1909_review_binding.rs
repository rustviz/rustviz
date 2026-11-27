fn consumed() {
    let s = 'blk: {
        if true { break 'blk String::new(); }
        String::from("skip")
    }; drop(s);
    std::hint::black_box(());
}

fn borrowed() {
    let mut s = 'blk: {
        if true { break 'blk String::new(); }
        String::from("skip")
    }; let r = &mut s;
    r.push('x');
}
