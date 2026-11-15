// #882: an `if` without `else` whose body always leaves (`break`,
// `continue`, `return`) ends a borrow on that path, while the paths that
// skip the body keep it: drawn as a conditional on the borrowed columns,
// with the end in its arm, as for an `if`/`else`.
fn main() {
    let v = vec![String::from("a"), String::from("b")];
    for a in &v {
        println!("{}", a);
        if v.len() > 1 {
            break;
        }
    }
    let t = String::from("t");
    for d in &v {
        let r = &t;
        if d.len() > 1 {
            println!("{}", r);
            break;
        }
        println!("{}", r);
    }
    for e in &v {
        let w = &t;
        if e.len() > 1 {
            continue;
        }
        println!("{} {}", w, e);
    }
    let mut n = 0;
    loop {
        let q = &t;
        if q.len() > n {
            break;
        }
        n += 1;
    }
    println!("{} {} {}", v.len(), t, first_len(&t));
}

fn first_len(s: &String) -> usize {
    let r = &s[..];
    if r.is_empty() {
        return 0;
    }
    r.len()
}
