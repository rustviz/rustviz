fn take(text: &mut String) { text.push('!'); }

fn look(number: &u32) { std::hint::black_box(*number); }

fn three_exits(choice: u8) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut third = String::from("third");
    let ra = &mut a;
    let rb = &mut b;
    take(loop {
        if choice == 0 { break ra; }
        if choice == 1 { break rb; }
        break &mut third;
    });
}

fn promoted() -> &'static u32 { &7 }

fn promoted_source(local: bool, first: bool) {
    let a = 42;
    let b = 99;
    let ra = if local { &a } else { promoted() };
    let rb = &b;
    look(loop {
        if first { break ra; } else { break rb; }
    });
}

fn inside_arm(run: bool, first: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let ra = &mut a;
    let rb = &mut b;
    if run {
        std::hint::black_box(());
        take(loop {
            if first { break ra; } else { break rb; }
        });
    } else {
        std::hint::black_box((&*ra, &*rb));
    }
}
