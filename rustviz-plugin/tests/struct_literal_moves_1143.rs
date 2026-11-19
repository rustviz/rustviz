// #1143: a struct literal moves (or copies, or borrows) what it is built
// from wherever it is written, not only as a `let` initializer.
struct B { name: String }
struct P { name: String, bio: String }
struct In { name: String }
struct Out { inner: In }
struct W<'a> { name: &'a String }

fn consume(_b: B) {}
fn take_p(_p: P) {}
fn look(_w: W) {}
fn take_vec(_v: Vec<B>) {}
fn take_opt(_o: Option<B>) {}

fn call_arg() {
    let a1 = String::from("a");
    consume(B { name: a1 });
}

fn method_arg() {
    let a2 = String::from("a");
    let mut v2: Vec<B> = Vec::new();
    v2.push(B { name: a2 });
    println!("{}", v2.len());
}

fn box_new() {
    let a3 = String::from("a");
    let b3 = Box::new(B { name: a3 });
    println!("{}", b3.name);
}

fn some_let() {
    let a4 = String::from("a");
    let o4 = Some(B { name: a4 });
    println!("{}", o4.is_some());
}

fn vec_let() {
    let a5 = String::from("a");
    let v5 = vec![B { name: a5 }];
    println!("{}", v5.len());
}

fn shorthand() {
    let name = String::from("a");
    consume(B { name });
}

fn base_in_call() {
    let a7 = String::from("a");
    let base = P { name: String::from("x"), bio: String::from("y") };
    take_p(P { name: a7, ..base });
    println!("{}", base.name);
}

fn field_into_call() {
    let p8 = B { name: String::from("a") };
    consume(B { name: p8.name });
}

fn ref_field_call() {
    let a9 = String::from("a");
    look(W { name: &a9 });
    println!("{}", a9);
}

fn in_branch(c: bool) {
    let a10 = String::from("a");
    if c {
        consume(B { name: a10 });
    }
}

fn in_loop() {
    let mut v11 = Vec::new();
    for i in 0..2 {
        let a11 = i.to_string();
        v11.push(B { name: a11 });
    }
    println!("{}", v11.len());
}

fn vec_arg() {
    let a12 = String::from("a");
    take_vec(vec![B { name: a12 }]);
}

fn some_arg() {
    let a13 = String::from("a");
    take_opt(Some(B { name: a13 }));
}

fn assign_some() {
    let a14 = String::from("a");
    let mut o14: Option<B> = None;
    println!("{}", o14.is_some());
    o14 = Some(B { name: a14 });
    println!("{}", o14.is_some());
}

fn tuple_elem() {
    let a15 = String::from("a");
    let t15 = (1, Some(B { name: a15 }));
    println!("{}", t15.0);
}

fn index_write() {
    let a16 = String::from("a");
    let mut v16 = vec![B { name: String::new() }];
    v16[0] = B { name: a16 };
    println!("{}", v16.len());
}

fn deref_write(slot: &mut B, a17: String) {
    let s17 = a17;
    *slot = B { name: s17 };
}

fn ret_some(a18: String) -> Option<B> {
    let s18 = a18;
    Some(B { name: s18 })
}

fn ret_nested(a19: String) -> Out {
    let s19 = a19;
    Out { inner: In { name: s19 } }
}

fn ret_vec(a20: String) -> Vec<B> {
    let s20 = a20;
    vec![B { name: s20 }]
}

fn ret_tuple(a21: String) -> (B, i32) {
    let s21 = a21;
    (B { name: s21 }, 1)
}

fn main() {
    call_arg();
    method_arg();
    box_new();
    some_let();
    vec_let();
    shorthand();
    base_in_call();
    field_into_call();
    ref_field_call();
    in_branch(true);
    in_loop();
    vec_arg();
    some_arg();
    assign_some();
    tuple_elem();
    index_write();
    let mut b = B { name: String::new() };
    deref_write(&mut b, String::from("a"));
    let _r1 = ret_some(String::new());
    let _r2 = ret_nested(String::new());
    let _r3 = ret_vec(String::new());
    let _r4 = ret_tuple(String::new());
}
