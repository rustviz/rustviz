struct Pair { a: String, b: String }
struct Outer { pair: Pair, tag: String }

fn main() {
    let mut p = Pair { a: String::from("old"), b: String::new() };
    p = Pair { a: String::new(), ..p };
    let mut outer = Outer { pair: p, tag: String::new() };
    if outer.tag.is_empty() {
        outer.pair = Pair { a: String::from("next"), ..outer.pair };
    }
}

fn choose(mut pair: Pair, left: bool) {
    pair = if left {
        Pair { a: String::new(), ..pair }
    } else {
        Pair { b: String::new(), ..pair }
    };
}
