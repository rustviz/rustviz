// #1457: a constructor around a conditional whose branches borrow
// (`Some(if c { &s } else { &u })`) gives k a value holding s's or u's
// reference. Both stay borrowed until k's last use, where both get
// their borrow back.
fn main() {
    let c = true;
    let s = String::from("s");
    let u = String::from("u");
    let k = Some(if c { &s } else { &u });
    println!("{:?}", k);

    // km moved into `if let`: r takes the borrow where the pattern
    // matches; on the other path km's borrow ends there (#1464).
    let mut m1 = String::from("m");
    let mut m2 = String::from("n");
    let km = Some(match s.len() {
        0 => &mut m1,
        _ => &mut m2,
    });
    if let Some(r) = km {
        r.push('!');
    }
    println!("{} {}", m1, m2);

    let r1 = &s;
    let r2 = &u;
    let kr = Some(if c { r1 } else { r2 });
    println!("{:?}", kr);
}
