// An arrow starts at the edge of its source dot, not at its centre.
// Arrows are drawn after dots, so a tail on the centre covered the
// middle of the dot and hovering there showed the arrow's tooltip
// instead of the dot's (#269).
//
// `let t = s;`: a straight arrow from s's dot ("s's resource is
// moved"). `let b = &a; let c = &a;`: two arrows on one line, so the
// second is drawn as a trapezoid that leaves its source dot diagonally.
fn main() {
    let s = String::from("hi");
    let t = s;
    let a = String::from("a");
    let b = &a; let c = &a;
    println!("{} {} {}", t, b, c);
}
