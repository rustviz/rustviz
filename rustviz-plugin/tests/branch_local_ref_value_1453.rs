// #1453: each branch declares its own reference and hands it over as
// the branch's value. y is a copy of whichever ran, so after the `match`
// y may point into s1 or s2: rustc keeps both borrowed until y's last
// use, where y gives both back. The branch's own `r` ends with its
// scope (its loan lives on in y).
fn main() {
    let s1 = String::from("hi");
    let s2 = String::from("yo");
    let n = s1.len();
    let y = match n {
        0 => {
            let r = &s1;
            r
        }
        _ => {
            let r = &s2;
            r
        }
    };
    println!("{:?}", y);

    // Different names, an `if`.
    let w = if n > 2 {
        let r1 = &s1;
        r1
    } else {
        let r2 = &s2;
        r2
    };
    println!("{:?}", w);

    // A value that holds the branch's reference.
    let h = match n {
        0 => {
            let p = &s1;
            Some(p)
        }
        _ => {
            let p = &s2;
            Some(p)
        }
    };
    println!("{:?}", h);

    // A constructor around the conditional.
    let k = Some(if n > 2 {
        let q = &s1;
        q
    } else {
        let q = &s2;
        q
    });
    println!("{:?}", k);

    // `let x;` given its value by the branches.
    let x;
    x = if n > 2 {
        let t = &s1;
        t
    } else {
        let t = &s2;
        t
    };
    println!("{:?}", x);
}
