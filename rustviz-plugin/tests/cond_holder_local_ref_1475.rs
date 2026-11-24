// #1475: a holder given a reference its branch declared (`let r = &mut
// s1; Some(r)`) holds r's loan. Moved into an `if let` (or a `match`),
// it hands that loan to the binding where the pattern matches, and its
// borrow ends at the `if let` where it doesn't: s1 and s2 are free again
// at the `println!` (main drew them still lent to y1 there).
fn main() {
    let mut s1 = String::from("hi");
    let mut s2 = String::from("yo");
    let n = s1.len();
    let y1 = if n > 2 {
        let r = &mut s1;
        Some(r)
    } else {
        let r = &mut s2;
        Some(r)
    };
    if let Some(x) = y1 {
        x.push('!');
    }
    println!("{} {}", s1, s2);

    let y2 = Some(if n > 2 {
        let r = &mut s1;
        r
    } else {
        let r = &mut s2;
        r
    });
    if let Some(x) = y2 {
        x.push('?');
    }
    println!("{} {}", s1, s2);

    let y3 = match n {
        0 => {
            let r = &mut s1;
            Some(r)
        }
        _ => {
            let r = &mut s2;
            Some(r)
        }
    };
    match y3 {
        Some(x) => x.push('.'),
        None => {}
    }
    println!("{} {}", s1, s2);
    shared(1);
    taken(1);
}

// A shared one: y4 is copied into x, and gives s1 / s2 back after x's
// last use, so both can be written after the `if let`.
fn shared(n: usize) {
    let mut s1 = String::from("hi");
    let mut s2 = String::from("yo");
    let y4 = if n > 2 {
        let r = &s1;
        Some(r)
    } else {
        let r = &s2;
        Some(r)
    };
    if let Some(x) = y4 {
        println!("{}", x);
    }
    s1.push('a');
    s2.push('b');
    println!("{} {}", s1, s2);
}

// `y5.take()` each time round: y5 holds the loan until the loop ends
// (main gave it back after x's first use, inside the loop).
fn taken(n: usize) {
    let mut s1 = String::from("hi");
    let mut s2 = String::from("yo");
    let mut y5 = if n > 2 {
        let r = &mut s1;
        Some(r)
    } else {
        let r = &mut s2;
        Some(r)
    };
    while let Some(x) = y5.take() {
        x.push('!');
    }
    println!("{} {}", s1, s2);
}
