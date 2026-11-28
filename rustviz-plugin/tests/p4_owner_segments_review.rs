#![allow(dead_code)]

struct Pair { a: String, b: String }
struct Holder<'a> { r: &'a String }

fn break_after_write(mut v: Vec<i32>) {
    for x in v.iter_mut() {
        if *x == 1 {
            *x += 1;
            v.push(3);
            break;
        }
        *x += 1;
    }
}

fn item_outlives_loop(mut items: Vec<i32>) {
    let mut saved = None;
    for item in &mut items {
        saved = Some(item);
        break;
    }
    if let Some(item) = saved {
        *item += 1;
    }
    println!("{}", items.len());
}

fn repoint_field(choose: bool, first: String, second: String) {
    let mut h = Holder { r: &first };
    let old = h.r;
    h.r = &second;
    if choose {
        println!("{old}");
    }
    println!("{}", h.r);
}

fn inspect(_: &Pair) {}

fn arm_update(choose: bool, base: Pair) {
    if choose {
        inspect(&base);
        let replacement = Pair {
            a: String::new(),
            ..base
            // Only base.b is moved by the aggregate statement.
        };
        println!("{} {}", base.a, replacement.b);
    } else {
        inspect(&base);
    }
}

fn sibling(outer: bool, inner: bool, s: String) {
    if outer { if inner { drop(s); } else { drop(s); }
        println!("moved");
    } else {
        println!("{s}");
    }
}

fn main() {
    break_after_write(vec![1]);
    item_outlives_loop(vec![1]);
}

fn same_row_successor(mut left: String, mut right: String, take_left: bool) {
    let l = &mut left;
    let r = &mut right;
    if take_left { l.push('!'); } else {
        r.push('!');
    }
}

fn partial_move_edge(pair: Pair, take_field: bool, choose: bool) {
    let shared = &pair.b;
    if take_field {
        let taken = pair.a;
        let selected = if choose { shared } else { &taken };
        println!("{selected}");
    } else {
        println!("{shared}");
    }
    println!("{}", pair.b);
}
