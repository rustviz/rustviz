// A binding inside the initializer of a `let` of the same name (`let n =
// match o { Some(n) => n, .. }`) gets a column of its own: on a shared
// column the outer n had no line from its initialization (#1201).
// A binding by reference into what a call's returned reference points to
// borrows what the call borrowed, not the function (#1222).
// (#1239: `let v = match o { Some(v) => v, None => String::new() };`
// panicked.)
fn get(o: &Option<String>) -> &Option<String> { o }
fn get_mut(o: &mut Option<String>) -> &mut Option<String> { o }
fn pick<'a>(a: &'a Option<String>, b: &'a Option<String>) -> &'a Option<String> {
    if a.is_some() { a } else { b }
}

fn arm(o: Option<usize>) -> usize {
    let s = String::from("s");
    let w = &s;
    let n = match o {
        Some(n) => n,
        None => return w.len(),
    };
    w.len() + n
}

fn arm_string(opt: Option<String>) {
    let s = String::from("s");
    let x = match opt {
        Some(x) => x,
        None => {
            drop(s);
            return;
        }
    };
    println!("{} {}", x, s);
}

// A `None` arm that doesn't leave: main panicked here (#1239).
fn arm_value(o: Option<String>) {
    let v = match o { Some(v) => v, None => String::new() };
    println!("{}", v);
}

fn if_let_init(o: Option<String>) {
    let y = if let Some(y) = o { y } else { String::new() };
    println!("{}", y);
}

fn block_init() {
    let b = {
        let b = String::from("b");
        b
    };
    println!("{}", b);
}

fn call_scrutinee() {
    let o = Some(String::from("a"));
    match get(&o) {
        Some(x) => println!("{}", x),
        None => {}
    }
    if let Some(z) = get(&o) {
        println!("{}", z);
    }
    let mut m = Some(String::from("m"));
    match get_mut(&mut m) {
        Some(mx) => mx.push('!'),
        None => {}
    }
    let a = Some(String::from("p"));
    let b = Some(String::from("q"));
    if let Some(pq) = pick(&a, &b) {
        println!("{}", pq);
    }
}

fn main() {
    println!("{}", arm(Some(1)));
    arm_string(Some(String::from("x")));
    arm_value(None);
    if_let_init(None);
    block_init();
    call_scrutinee();
}
