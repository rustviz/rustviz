// An `if let` binding is dropped where its branch leaves the loop
// iteration (#630): `a` at the `break` in `f` (an `if let` without
// `else`, drawn as a branch because it moves out of `o`), `b` and
// `c` at the `continue` in `g`. Neither branch reaches its closing
// brace.

fn f(v: Vec<Option<String>>) -> usize {
    let mut n = 0;
    for o in v {
        if let Some(a) = o {
            n += a.len();
            break;
        }
        n += 1;
    }
    n
}

fn g(v: Vec<Option<String>>) -> usize {
    let mut n = 0;
    for o in v {
        if let Some(b) = o {
            let c = String::from("c");
            n += b.len() + c.len();
            continue;
        } else {
            n += 1;
        }
        n += 2;
    }
    n
}

fn main() {
    println!("{} {}", f(vec![None]), g(vec![None]));
}
