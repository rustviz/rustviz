// #1225: a constructor around an `if` / `match` (`Some(if c { d } else
// { e })`) takes nothing itself (#768): the variable it initializes or is
// assigned to gets each branch's value in its branch, as for `let x = if
// …`. d moves only on the then-path, e only on the else-path.
fn let_if(c: bool) {
    let d = String::from("d");
    let e = String::from("e");
    let _x = Some(if c { d } else { e });
}

fn let_match(n: i32) {
    let f = String::from("f");
    let g = String::from("g");
    let _y = Some(match n {
        0 => f,
        _ => g,
    });
}

fn assign(c: bool) {
    let h = String::from("h");
    let i = String::from("i");
    let mut z = None;
    z = Some(if c { h } else { i });
    println!("{:?}", z);
}

fn deferred(c: bool) {
    let j = String::from("j");
    let k = String::from("k");
    let w;
    w = Some(if c { j } else { k });
    println!("{:?}", w);
}

fn main() {
    let_if(true);
    let_match(1);
    assign(false);
    deferred(true);
}
