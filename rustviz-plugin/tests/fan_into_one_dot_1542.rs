// #1542: each arrow into one dot gets its own approach: one from below at a
// one-row dip's angle (56 degrees), the next level along the dot's row from
// a side no other arrow takes, or from below on the dot's far side.

// Two loans of `longest` given back into s3's and s2's dots, by r and r2.
// No side of s3's dot is free: a level arrow hops it (so neither level
// nor from above), and from below on the far side crosses another arrow.
// So this fan keeps main's drawing (no fan may cross more than on main,
// the maintainer's ruling on #1605); KNOWN_INVARIANT_FAILURES lists it.
fn longest<'a>(a: &'a String, b: &'a String) -> &'a String { if a.len() > b.len() { a } else { b } }
fn longest_copied() {
    let s = String::from("hi");
    let s2 = String::from("ab");
    let s3 = s;
    let r = longest(&s3, &s2);
    let r2 = r;
}

// Three returns into r's `if let` dot: one level, one from below, one from
// the other side.
fn three_into_one(mut m1: String, mut m2: String, c: bool) {
    let kc = if c { Some(&mut m1) } else { Some(&mut m2) };
    if let Some(r) = kc {
        r.push('?');
    }
    println!("{} {}", m1, m2);
}
