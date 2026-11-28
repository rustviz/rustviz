#![allow(dead_code)]

struct Pair { a: String, b: String }

fn inspect(_: &Pair) {}

fn stacked(mut pair: Pair) {
    let shared = &pair; inspect(shared); let edit = &mut pair.a;
    edit.push('!');
    inspect(&pair);
}

fn branch(mut left: String, mut right: String, choose_left: bool) {
    let edit = if choose_left {
        let chosen = &mut left;
        chosen.push('l');
        chosen
    } else {
        let chosen = &mut right;
        chosen.push('r');
        chosen
    };
    edit.push('!');
    assert!(left.ends_with('!') || right.ends_with('!'));
}

fn edge(mut owned: String, chosen_path: bool) {
    let writer = &mut owned;
    if chosen_path {
        writer.push('!');
        println!("wrote");
    } else {
        println!("skipped");
    }
    println!("{owned}");
}
