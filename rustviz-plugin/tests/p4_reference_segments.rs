#![allow(dead_code)]

struct Pair { left: i32, right: i32 }

fn regions<'a>(mut r: &'a mut Pair, next: &'a mut Pair) {
    let part = &mut r.left;
    r.right += 1;
    *part += 2; r = next;
    r.right += 3;
}

fn slot<'a>(mut r: &'a mut Pair, next: &'a mut Pair) {
    let held = &mut r;
    (**held).right += 1;
    r = next;
    r.right += 2;
}

fn repoint<'a>(mut r: &'a mut Pair, next: &'a mut Pair) {
    let old = &mut *r; r = next;
    r.right += 1;
    old.left += 1;
}

fn arms(r: &mut Pair, choice: u8) {
    match choice {
        0 => {
            let field = &mut r.left;
            r.right += 1;
            *field += 2;
        }
        1 => {
            let shared = &*r;
            let read = shared.right;
            assert_eq!(shared.left, read);
        }
        _ => { r.right += 4; }
    }
    r.right += 8;
}

fn two_stores<'a>(mut r: &'a mut Pair, next: &'a mut Pair, last: &'a mut Pair) {
    let before = &mut *r;
    r = next; let middle = &mut r.left; r = last;
    r.left += 1;
    *middle += 1;
    before.left += 1;
}

fn joined(r: &mut Pair, other: &mut Pair, flag: bool) {
    let dst: &mut Pair;
    if flag { dst = &mut *r; } else { dst = &mut *other; }
    dst.left += 1;
    r.right += 1;
}

fn joined_fields(r: &mut Pair, other: &mut Pair, flag: bool) {
    let dst: &mut i32;
    if flag { dst = &mut r.left; } else { dst = &mut other.left; }
    r.right += 1;
    *dst += 2;
}

fn replace_after_join<'a>(
    mut slot: &'a mut Pair, spare: &'a mut Pair, next: &'a mut Pair, flag: bool,
) {
    let kept = if flag { &mut *slot } else { &mut *spare };
    slot = next;
    slot.left += 1;
    kept.left += 2;
}

fn nested_slot(slot: &mut &mut i32) {
    let kept = &mut **slot;
    std::hint::black_box(&0);
    std::hint::black_box(&1);
    std::hint::black_box(&2);
    *kept += 2;
}

enum Payload { Only(i32) }

fn payload_field(input: &mut Payload) {
    let Payload::Only(ref mut number) = *input;
    *number += 1;
}

struct Bag { items: Vec<usize>, other: usize }

fn reserve_field(r: &mut Bag) {
    r.items.push(
        r.items.len()
    );
    r.other += 1;
}

fn yield_field(r: &mut Bag) {
    let mut iterator = r.items.iter_mut();
    let kept = iterator.next().unwrap();
    drop(iterator);

    r.other += 1;
    *kept += 1;
}


fn sibling_entry(r: &mut Pair, flag: bool) {
    if flag {
        let shared = &*r;
        std::hint::black_box(shared.left);
    } else {
        r.left += 1;
    }
}

fn imaginary_replacement<'a>(mut slot: &'a mut Pair, next: &'a mut Pair) {
    let old = &mut *slot;
    slot = next;
    if let _ = 0 {
        slot.left += 1;
    } else {
        slot.right += 1;
    }
    old.left += 2;
}

fn same_row_carriers(r: &mut Pair) {
    let issued = &mut *r; let later = issued;
    later.left += 1;
}

fn break_value(flag: bool, left: &mut i32, right: &mut i32) {
    let picked: &mut i32 = loop {
        if flag { break &mut *left; } else { break &mut *right; }
    };
    *picked += 1;
}

fn no_break(independent: &mut i32) {
    let own_loan = &mut *independent;
    *own_loan += 1;
}

fn take(r: &mut i32) -> &mut i32 { r }

fn break_call(flag: bool, call_left: &mut i32, call_right: &mut i32) {
    let held = take(loop {
        if flag { break &mut *call_left; } else { break &mut *call_right; }
    });
    *held += 1;
}

fn sibling_live(r: &mut Pair, flag: bool) {
    let held;
    if flag {
        held = Some(&*r);
    } else {
        r.left += 1;
        held = None;
    }
    std::hint::black_box(held);
}
