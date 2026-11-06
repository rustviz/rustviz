// Writes through a reference inside `if` / `match` arms (#147),
// including the `*s = …` form in both arms of an if/else, which used
// to panic in the renderer: the reference wasn't counted as live in
// the branch, so the write's arrow had no event to attach to.
struct P {
    x: i32,
    name: String,
}

fn set(p: &mut P, flag: bool) {
    if flag {
        p.x = 5;
        p.name = String::from("b");
    } else {
        p.x = 6;
    }
}

fn pick(p: &mut P, o: Option<String>) {
    match o {
        Some(v) => {
            p.name = v;
        }
        None => {
            p.x = 0;
        }
    }
}

fn both(s: &mut String, flag: bool) {
    if flag {
        *s = String::from("b");
    } else {
        *s = String::from("c");
    }
}

fn main() {
    let mut q = P { x: 1, name: String::from("a") };
    set(&mut q, true);
    pick(&mut q, Some(String::from("z")));
    let mut s = String::new();
    both(&mut s, false);
    println!("{} {}", q.name, s);
}
