// #1477: `v = if c { let r = &a; vec![r] } else { let r = &b; vec![r] };`
// pairs each branch's value with v in that branch, while its `r` is
// still that branch's: v borrows a in the first and b in the second, and
// gives both back at its last use (main drew both branches borrowing b).
// v's old Vec is dropped as the new one arrives; its loan of b ended at
// its last use before, the `println!` on line 12.
fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut v = vec![&b];
    println!("{:?}", v);
    v = if c {
        let r = &a;
        vec![r]
    } else {
        let r = &b;
        vec![r]
    };
    println!("{:?}", v);

    let n = a.len();
    let mut w = vec![&a];
    println!("{:?}", w);
    w = match n {
        0 => {
            let q = &b;
            vec![q]
        }
        _ => {
            let q = &a;
            vec![q]
        }
    };
    println!("{:?}", w);
    muts(true);
}

// An `Option<&mut String>` (not Copy): the first branch's r is moved into
// y, which borrows s1, not s2 (main took the lender of the second
// branch's r, the one `r` named after the `if`).
fn muts(c: bool) {
    let mut s1 = String::from("hi");
    let mut s2 = String::from("yo");
    let mut y = None;
    println!("{:?}", y);
    y = if c {
        let r = &mut s1;
        Some(r)
    } else {
        let r = &mut s2;
        r.push('?');
        None
    };
    if let Some(x) = y {
        x.push('!');
    }
    println!("{} {}", s1, s2);
}
