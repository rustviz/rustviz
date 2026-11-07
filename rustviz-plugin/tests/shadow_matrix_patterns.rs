// Shadowing matrix, part 3: pattern bindings that reuse a name.

// `if let Some(s) = s`: the binding is a new variable inside the `if`.
fn if_let_same_name() {
    let s = Some(String::from("a"));
    if let Some(s) = s {
        println!("{}", s);
    }
}

// `match o { Some(o) => … }`.
fn match_same_name() {
    let o: Option<String> = Some(String::from("o"));
    match o {
        Some(o) => println!("{}", o),
        None => println!("none"),
    }
}

// `while let Some(x) = it.next()` with an outer `x` used after the loop.
fn while_let_outer() {
    let x = 10;
    let v = vec![1, 2, 3];
    let mut it = v.iter();
    while let Some(x) = it.next() {
        println!("{}", x);
    }
    println!("{}", x);
}

// `for y in ys` with an outer String `y` used after the loop.
fn for_outer() {
    let y = String::from("outer");
    let ys = vec![1, 2, 3];
    for y in ys {
        println!("{}", y);
    }
    println!("{}", y);
}

// `if let … else if let …` binding the same name: two variables.
fn else_if_let(a: Option<i32>, b: Option<i32>) {
    if let Some(z) = a {
        println!("{}", z);
    } else if let Some(z) = b {
        println!("{}", z);
    }
}

// A swap by destructuring: the new `f`, `g` take the old `g`, `f`.
fn tuple_swap() {
    let f = String::from("f");
    let g = String::from("g");
    let (f, g) = (g, f);
    println!("{} {}", f, g);
}

// A struct pattern binds a field into a name already in use.
struct P {
    n: String,
    k: i32,
}
fn struct_pattern() {
    let n = 1;
    let p = P { n: String::from("pn"), k: 2 };
    let P { n, .. } = p;
    println!("{} {}", n, p.k);
}

// A nested match whose arms bind the same name as the outer arm.
fn nested_match(a: Option<i32>, b: Option<i32>) {
    match a {
        Some(w) => match b {
            Some(w) => println!("{}", w),
            None => println!("{}", w),
        },
        None => {}
    }
}

fn main() {
    if_let_same_name();
    match_same_name();
    while_let_outer();
    for_outer();
    else_if_let(None, Some(2));
    tuple_swap();
    struct_pattern();
    nested_match(Some(1), Some(2));
}
