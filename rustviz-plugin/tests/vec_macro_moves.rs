// `vec![…]` moves the variables written as its elements into the new
// Vec, like an array literal (#297): `s` and `t` own nothing after
// line 9. Also through `vec![w; 2]`, as a call argument, and as a
// function's result.

fn keep(v: Vec<String>) -> usize { v.len() }

fn wrap(q: String) -> Vec<String> {
    vec![q]
}

fn main() {
    let s = String::from("a");
    let t = String::from("b");
    let v = vec![s, t];
    let w = String::from("w");
    let h = vec![w; 2];
    let m = String::from("m");
    let n = keep(vec![m]);
    let r = wrap(String::from("r"));
    println!("{:?} {:?} {} {:?}", v, h, n, r);
}
