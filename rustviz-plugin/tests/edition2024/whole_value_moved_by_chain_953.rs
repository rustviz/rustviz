// #953: a let-chain's condition that moves a whole value (`&& { let w =
// s; … }`). On the path where the condition moves s and a later part
// then fails (or where the moving part is the last one and the body
// runs), s is gone although the branch that ran didn't move it. The
// join said "s was moved in at least one branch above … If the branch
// that ran didn't move it, its resource is dropped when s goes out of
// scope", which is false on those paths. Checked with a `Drop`-printing
// type.

fn take(_s: &mut String) {}

fn main() {
    let o = Some(3);

    // The issue's repro: the condition's last part moves s.
    let s = String::from("s");
    if let Some(x) = o && x > 5 && { let ws = s; drop(ws); true } {
        println!("y");
    }

    // A part after the move can fail; t is assigned again later.
    let mut t = String::from("t");
    if let Some(x) = o && { let wt = t; drop(wt); true } && x > 5 {
        println!("y");
    }
    t = String::from("t2");
    println!("{}", t);

    // With an else branch.
    let u = String::from("u");
    if let Some(x) = o && { let wu = u; drop(wu); x > 1 } && x > 5 {
        println!("y");
    } else {
        println!("n");
    }

    // A `&mut` reference moved by the condition owns nothing to drop.
    let mut v = String::from("v");
    let r = &mut v;
    if let Some(x) = o && { let wr = r; take(wr); true } && x > 5 {
        println!("y");
    }

    // Only the body moves q: the join keeps the branch wording.
    let q = String::from("q");
    if let Some(x) = o && x > 5 {
        drop(q);
    }
}
