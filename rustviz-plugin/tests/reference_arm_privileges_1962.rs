fn match_arms(r: &mut i32, choice: u8) {
    let held = match choice {
        0 => {
            let child = &mut *r;
            std::hint::black_box(&*child);
            Some(child)
        }
        1 => {
            *r += 1;
            std::hint::black_box(*r);
            None
        }
        _ => {
            *r += 2;
            std::hint::black_box(*r);
            None
        }
    };
    std::hint::black_box(held);
}

fn let_else(r: &mut i32, choice: Option<()>) {
    let child = &mut *r;
    let Some(()) = choice else {
        *r += 1;
        std::hint::black_box(*r);
        return;
    };
    *child += 2;
}

fn loop_arms(r: &mut i32, flag: bool) {
    let held = loop {
        let choice = if flag {
            let child = &mut *r;
            std::hint::black_box(&*child);
            Some(child)
        } else {
            *r += 1;
            std::hint::black_box(*r);
            None
        };
        break choice;
    };
    std::hint::black_box(held);
}

enum Loan<'a> {
    Exclusive(&'a mut i32),
    Shared(&'a &'a mut i32),
}

fn alternative_nested(slot: &mut &mut i32, flag: bool) {
    let held = if flag {
        Loan::Exclusive(&mut **slot)
    } else {
        Loan::Shared(&*slot)
    };
    std::hint::black_box(held);
}

fn nested_reservation(slot: &mut &mut Vec<i32>) {
    (**slot).push({
        let len = (**slot).len();
        len as i32
    });
}

fn bind_on_success(visited: Option<&i32>) {
    let Some(p) = visited else { return };
    std::hint::black_box(*p);
}

fn left_value() -> &'static i32 { &1 }
fn right_value() -> &'static i32 { &2 }

fn call_result(flag: bool) {
    let result: &'static i32 = match flag {
        true => left_value(),
        false => right_value(),
    };
    std::hint::black_box(*result);
}
