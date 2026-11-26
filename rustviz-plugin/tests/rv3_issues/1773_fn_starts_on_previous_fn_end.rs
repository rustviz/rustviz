struct Pair {
    a: String,
    b: String,
}

fn first_fn(ff: Pair) { drop(ff.a);
} fn second_fn(ff: Pair) { drop(ff.b);
    let n = ff.a.len();
}

fn main() {
    first_fn(Pair { a: String::new(), b: String::new() });
    second_fn(Pair { a: String::new(), b: String::new() });
}
