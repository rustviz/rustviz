// #663: a `break` or `continue` leaves the loop body, and `r` / `q`,
// declared in it, with it: their borrows end on that path. `k`, made
// before the loop and used after it, stays borrowed past the `break`.
fn brk(v: Vec<bool>) -> usize {
    let t = String::from("t");
    let mut n = 0;
    for d in v {
        let r = &t;
        if d {
            break;
        } else {
            println!("d");
        }
        n += r.len();
    }
    n
}

fn cont(v: Vec<bool>) -> usize {
    let u = String::from("u");
    let mut n = 0;
    for d in v {
        let q = &u;
        if d {
            continue;
        } else {
            println!("e");
        }
        n += q.len();
    }
    n
}

fn kept(v: Vec<bool>) -> usize {
    let w = String::from("w");
    let k = &w;
    let mut n = 0;
    for d in v {
        if d {
            n += 3;
            break;
        } else {
            n += 1;
        }
        n += 2;
    }
    n + k.len()
}

fn main() {
    println!("{} {} {}", brk(vec![true]), cont(vec![false]), kept(vec![true]));
}
