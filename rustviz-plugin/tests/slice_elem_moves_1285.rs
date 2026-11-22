// #1285: a slice pattern that leaves some elements in the array moves only
// the elements it binds: `pair[0]`, not all of `pair`. pair can't be used
// as a whole after it (E0382, also `pair[1]`), but pair[1] stays in it and
// is dropped at its scope end. main said "pair's resource is moved", then
// "only pair[1] is dropped".
fn match_arm() {
    let pair = [String::from("a"), String::from("b")];
    match pair {
        [c, _] => println!("{}", c),
    }
}
fn let_rest() {
    let three = [String::from("a"), String::from("b"), String::from("c")];
    let [e, ..] = three;
    println!("{}", e);
}
fn if_let_refutable() {
    let opts = [Some(String::from("a")), None];
    if let [Some(a), _] = opts {
        println!("{}", a);
    }
}
fn arms_with_guard(n: i32) {
    let arr = [String::from("a"), String::from("b"), String::from("c")];
    match arr {
        [a, _, _] if n > 2 => println!("{}", a),
        [_, b, _] => println!("{}", b),
    }
}
fn let_else() {
    let opts = [Some(String::from("a")), None];
    let [Some(c), _] = opts else { return };
    println!("{}", c);
}
fn tail_binding() {
    let pair = [String::from("a"), String::from("b")];
    let [_, last] = pair;
    println!("{}", last);
}
fn param([p, _]: [String; 2]) {
    println!("{}", p);
}
fn param_rest([first, ..]: [String; 3]) {
    println!("{}", first);
}
fn main() {
    match_arm();
    let_rest();
    if_let_refutable();
    arms_with_guard(3);
    let_else();
    tail_binding();
    param([String::new(), String::new()]);
    param_rest([String::new(), String::new(), String::new()]);
}
