// #1435: a value that holds a borrow (an `Option<&T>`, an array or a Vec of
// references) bound from an `if` / `match` whose branch builds it around a
// borrow holds that borrow, as rustc sees it (`s.push('x')` while t is live
// is E0502). It was drawn borrowing nothing: no arrow from s, no borrow end
// at its last use. Each branch now draws its own borrow, as a plain `let t
// = Some(&s);` does. Also `else if`, nested, a variable the branch
// declares, a later assignment, a struct field's value, `if let`, a guard,
// and the `break` values of a `loop`.
#[derive(Debug)]
struct H<'a> {
    o: Option<&'a String>,
}

fn some_if(c: bool) {
    let s = String::from("s");
    let t = if c {
        Some(&s)
    } else {
        None
    };
    println!("{:?}", t);
}

fn array_if(c: bool) {
    let s = String::from("s");
    let u = String::from("u");
    let a = if c { [&s] } else { [&u] };
    println!("{:?}", a);
}

fn vec_if(c: bool) {
    let s = String::from("s");
    let u = String::from("u");
    let v = if c {
        vec![&s]
    } else {
        vec![&u]
    };
    println!("{:?}", v);
}

fn match_arms(c: bool) {
    let s = String::from("s");
    let u = String::from("u");
    let m = match c {
        true => Some(&s),
        false => Some(&u),
    };
    println!("{:?}", m);
}

fn else_if(n: i32) {
    let s = String::from("s");
    let u = String::from("u");
    let e = if n > 0 {
        Some(&s)
    } else if n < 0 {
        Some(&u)
    } else {
        None
    };
    println!("{:?}", e);
}

fn nested(c: bool, d: bool) {
    let s = String::from("s");
    let u = String::from("u");
    let n = if c {
        if d { Some(&s) } else { None }
    } else {
        Some(&u)
    };
    println!("{:?}", n);
}

fn branch_local(c: bool) {
    let s = String::from("s");
    let q = if c {
        let y = &s;
        Some(y)
    } else {
        None
    };
    println!("{:?}", q);
}

fn assigned(c: bool) {
    let s = String::from("s");
    let u = String::from("u");
    let t;
    t = if c { Some(&s) } else { None };
    let mut m = None;
    println!("{:?}", m);
    m = if c { Some(&u) } else { None };
    println!("{:?} {:?}", t, m);
}

fn field_value(c: bool) {
    let s = String::from("s");
    let h = H { o: if c { Some(&s) } else { None } };
    println!("{:?}", h);
}

fn if_let_guard(o: Option<i32>, c: bool) {
    let s = String::from("s");
    let g = if let Some(_) = o { Some(&s) } else { None };
    let w = match c {
        true if s.len() > 0 => Some(&s),
        _ => None,
    };
    println!("{:?} {:?}", g, w);
}

fn loop_breaks(c: bool) {
    let s = String::from("s");
    let u = String::from("u");
    let l = loop {
        break Some(&u);
    };
    let k = loop {
        if c {
            break Some(&s);
        }
        break Some(&u);
    };
    println!("{:?} {:?}", l, k);
}

fn main() {
    some_if(true);
    array_if(true);
    vec_if(true);
    match_arms(true);
    else_if(1);
    nested(true, true);
    branch_local(true);
    assigned(true);
    field_value(true);
    if_let_guard(Some(1), true);
    loop_breaks(true);
}
