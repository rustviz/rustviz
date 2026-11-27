fn return_selected(s: String, t: String) -> String {
    'b: {
        if true { break 'b s; }
        { t }
    }
}

fn copy_selected(a: i32, b: i32) -> i32 {
    let z = 'blk: {
        if true { break 'blk a; }
        b
    };
    z
}

fn borrow_selected(a: String, b: String) {
    let z = 'blk: { if true { break 'blk &a; } &b };
    std::hint::black_box(z);
}

fn selected_move(s: String, t: String) {
    let z = 'blk: {
        if true { break 'blk s; }
        t
    };
    drop(z);
}
