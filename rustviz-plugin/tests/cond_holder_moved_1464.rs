// #1464: a holder whose branches each borrow (on lines of their own)
// moved into an `if let`: r takes the borrow on the path where the
// pattern matches; on the other path km's borrow ends at the `if let`.
// m1 and m2 are free again after it (the `println!` compiles).
fn main() {
    let s = String::from("s");
    let mut m1 = String::from("m");
    let mut m2 = String::from("n");
    let c = s.len() > 0;
    let km = match s.len() {
        0 => Some(&mut m1),
        _ => Some(&mut m2),
    };
    if let Some(r) = km {
        r.push('!');
    }
    println!("{} {}", m1, m2);

    let kc = Some(if c {
        &mut m1
    } else {
        &mut m2
    });
    if let Some(r) = kc {
        r.push('?');
    }
    println!("{} {}", m1, m2);

    let ki = if c {
        Some(&mut m1)
    } else {
        Some(&mut m2)
    };
    if let Some(r) = ki {
        r.push('.');
    }
    println!("{} {}", m1, m2);
}
