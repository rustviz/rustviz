#![allow(unreachable_code, unused_assignments, unused_variables)]

fn consume<T>(value: T) { drop(value); }
fn element(value: String) -> String { value }
struct Part { payload: String }

fn operands(s: String, b: Box<String>, p: Part) {
    consume(vec![*b]);
    consume(vec![p]);
    consume(vec![panic!("stop"), element(s)]);
}

fn ref_operand(r: &mut i32) {
    consume(vec![r, panic!("stop")]);
}

fn replacement(mut v: Vec<String>, s: String) {
    v = vec![
        s,
        v[0].clone(),
    ];
    drop(v);
}

fn interrupted(s: String, count: Option<usize>) -> Option<()> {
    let v = vec![s; count?];
    drop(v);
    Some(())
}

fn empty(s: String) {
    let v = vec![s; 0];
    assert!(v.is_empty());
}

fn opposite(s: String) {
    let v = vec![s, (|| -> String { panic!("stop") })()];
    consume(v);
}

fn same_row(x: i32) {
    let v = vec![x, (|| -> i32 { panic!("stop") })(), x];
    consume(v);
}

fn literal(s: String) { (move || drop(s))(); }

fn maybe_call(s: String, arg: Option<()>) -> Option<()> {
    let f = |_: ()| drop(s);
    f(arg?);
    Some(())
}

fn no_call<F: FnOnce(i32)>(f: F) { f(return); }

fn escapes() {
    let mut n = 0;
    let mut r = &mut n;
    let x;
    match &mut r {
        s => { x = s; }
    }
    **x += 1;
    assert_eq!(*r, 1);
}

fn main() { escapes(); empty(String::new()); }

fn return_part(parcel: Part) -> Vec<Part> {
    vec![parcel]
}
