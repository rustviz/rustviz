// #1411: a struct or tuple reassigned from its own field. The right-hand
// side runs first and moves `p.b` into the new value, which the
// assignment puts into `p`: `p.b` holds the same String again, so its
// column goes on (it used to end there, and the later move of `p.b` was
// on a dead column, State::Invalid).
struct Pair {
    a: String,
    b: String,
}

fn mut_gen<T>(_t: &mut T) {}

fn one_field() {
    let mut p = Pair { a: String::from("a"), b: String::from("b") };
    p = Pair { a: String::from("c"), b: p.b };
    let Pair { a, b } = p;
    println!("{} {}", a, b);
}

fn both_fields() {
    let mut q = Pair { a: String::from("a"), b: String::from("b") };
    q = Pair { a: q.a, b: q.b };
    mut_gen(&mut q);
}

fn tuple() {
    let mut t = (String::from("a"), String::from("b"));
    t = (String::from("c"), t.1);
    let (x, y) = t;
    println!("{} {}", x, y);
}

fn main() {
    one_field();
    both_fields();
    tuple();
}
