// #1126: `S { x: 2, ..s1 }` takes s1's reference field too: a `&mut`
// field is moved (s2.r gets s1.r's borrow), a `&` field is copied.
struct S<'a> { x: i32, r: &'a mut String }
struct T<'a> { x: i32, r: &'a String }

fn consume_s(_s: S) {}

fn by_mut() {
    let mut a = String::from("a");
    let s1 = S { x: 1, r: &mut a };
    let s2 = S { x: 2, ..s1 };
    let _spacer = 1;
    s2.r.push('!');
    println!("{}", s1.x + s2.x);
}

fn by_shared() {
    let b = String::from("b");
    let t1 = T { x: 1, r: &b };
    let t2 = T { x: 2, ..t1 };
    let _spacer = 1;
    println!("{} {}", t2.r, t1.x + t2.x);
}

fn into_call() {
    let mut c = String::from("c");
    let u1 = S { x: 1, r: &mut c };
    consume_s(S { x: 2, ..u1 });
    c.push('!');
}

fn main() {
    by_mut();
    by_shared();
    into_call();
}
