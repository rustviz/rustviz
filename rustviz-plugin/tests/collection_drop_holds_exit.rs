// #542 at the early exits #410 and #430 draw: a collection dropped
// where a `return`, a panic, a `break` or a `?` leaves says it drops
// what it holds, like its drop at the end of its scope.
use std::collections::HashMap;

fn f(x: u32) -> usize {
    let v: Vec<String> = vec![String::from("a")];
    let o: Option<String> = Some(String::from("b"));
    let m: HashMap<String, u32> = HashMap::new();
    match x {
        0 => return 0,
        1 => panic!("one"),
        _ => {}
    }
    v.len() + o.map_or(0, |s| s.len()) + m.len()
}

fn g(xs: &[u32]) -> usize {
    let mut n = 0;
    for x in xs {
        let w: Vec<String> = Vec::new();
        if *x == 0 {
            break;
        } else {
            n += w.len();
        }
    }
    n
}

fn h(s: &str) -> Result<usize, std::num::ParseIntError> {
    let names: Vec<String> = vec![String::from("a")];
    let n: usize = s.parse()?;
    Ok(n + names.len())
}

fn main() {
    f(2);
    g(&[1, 0]);
    let _ = h("1");
}
