// #1542: each arrow into one dot gets its own approach: one from below at a
// one-row dip's angle (56 degrees), the next level along the dot's row from
// a side no other arrow takes, or from below on the dot's far side.
// (A fan with no such side keeps main's drawing:
// fan_into_one_dot_keeps_main_1542.)

// Three arrows into r's `if let` dot (kc's reference moved into r, and the
// loan from m1 or m2 it holds): one level, one from below, one from past it.
fn three_into_one(mut m1: String, mut m2: String, c: bool) {
    let kc = if c { Some(&mut m1) } else { Some(&mut m2) };
    if let Some(r) = kc {
        r.push('?');
    }
    println!("{} {}", m1, m2);
}
