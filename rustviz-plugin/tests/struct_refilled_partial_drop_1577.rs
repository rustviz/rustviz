// A struct moved as a whole, given a new value, then taken apart in one
// arm (#1577): at scope end only what no arm moved out of the new value
// is dropped, with a drop flag. The corpus checker's
// `drop-without-value` used to flag this scope end, seeing the whole
// move above and not the new value after it.

struct Pair {
    a: String,
    b: String,
}

impl Pair {
    fn into_a(self) -> String {
        self.a
    }
}

fn take(_s: String) {}

fn dropped_then_refilled(c: bool) {
    let mut p = Pair { a: String::new(), b: String::new() };
    drop(p);
    p = Pair { a: String::from("x"), b: String::from("y") };
    if c {
        let Pair { a: s4, b: s5 } = p;
        println!("{} {}", s4, s5);
    }
}

fn consumed_then_refilled(s: &str) {
    let mut p = Pair { a: String::from("a"), b: String::from("b") };
    take(p.into_a());
    p = Pair { a: String::from("ab"), b: String::from("ab") };
    match s {
        "hi" | "ab" => {
            let Pair { a: s5, b: s6 } = p;
            println!("{} {}", s5, s6);
        }
        _ => {}
    }
}

fn main() {
    dropped_then_refilled(true);
    consumed_then_refilled("hi");
}
