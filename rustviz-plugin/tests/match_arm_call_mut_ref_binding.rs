// Same as match_arm_macro_ref_binding, with a mutable borrow and a
// method call as the arm's one-line body (#371).

fn main() {
    let mut opt = Some(String::from("hi"));
    match &mut opt {
        Some(s) => s.push('!'),
        None => println!("none"),
    }
    println!("{:?}", opt);
}
