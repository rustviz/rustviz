pub fn first(mut v: Vec<String>) {
    let maybe = v.first();
    std::hint::black_box(maybe);
    v.clear();
}

pub fn first_mut(mut v: Vec<String>) {
    let maybe = v.first_mut();
    std::hint::black_box(&maybe);
    v.clear();
}

pub fn borrow_option(mut v: Vec<String>) {
    let o = v.first();
    let r = &o;
    std::hint::black_box(r);
    v.clear();
}

pub fn blank_before_assignment(mut b: String) {
    let mut h = None;
    std::hint::black_box(h);

    h = Some(&b);
    std::hint::black_box(h);
    b.clear();
}

pub fn sibling_bindings(v: Vec<String>) {
    {
        let x = v.first();
        std::hint::black_box(x);
    }
    {
        let x = 1u32;
        std::hint::black_box(x);
    }
}

pub fn join_after_break(mut a: String, take_borrow: bool) {
    let mut q = None;
    'outer: {
        if take_borrow {
            q = Some(&a);
            break 'outer;
        }
        {
            a.clear();
        }
    }
    std::hint::black_box(q);
    a.clear();
}

struct Holder<'a> { entry: Option<&'a String> }

pub fn option_field(mut v: Vec<String>) {
    let h = Holder { entry: v.first() };
    std::hint::black_box(h.entry);
    v.clear();
}
