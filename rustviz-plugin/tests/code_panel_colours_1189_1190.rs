// #1189: every variable with a column is coloured in the code panel,
// wherever it is written: destructuring lets, array literals, `arr[i]`,
// closure bodies, calls of a closure variable, casts, match guards,
// let-else. #1190: a field name (`a` in `p.a`, `P { a: .. }` building
// p) gets the colour of its field column.
struct P { a: String, b: String }

fn tuples(x: String, y: String) {
    let (s, t) = (x, y);
    let p = P { a: s, b: t };
    let P { a, b } = p;
    let arr = [a, b];
    let first = &arr[0];
    let f = |k: usize| first.len() + k;
    let n = f(1) as u64;
    println!("{}", n);
}

fn fields() {
    let q = P { a: String::from("a"), b: String::from("b") };
    let x = q.a;
    println!("{}", x);
    let y = q.b;
    println!("{}", y);
}

fn guard_and_else(o: Option<usize>, lim: usize, v: Vec<String>) {
    match o {
        Some(n) if n > lim => println!("{}", n),
        _ => {}
    }
    let [w, ..] = <[String; 1]>::try_from(v).ok().unwrap() else { return };
    println!("{}", w);
    let Some(m) = o else { return };
    println!("{}", m);
}

fn main() {
    tuples(String::new(), String::new());
    fields();
    guard_and_else(Some(1), 0, vec![String::new()]);
}
