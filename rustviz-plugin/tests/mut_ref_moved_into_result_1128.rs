fn id<T>(x: T) -> T { x }
struct W<T>(T);
fn through_id() {
    let mut s = String::from("a");
    let r1 = &mut s;
    let k1 = id(r1);
    k1.push('!');
}
fn into_some() {
    let mut s = String::from("b");
    let r2 = &mut s;
    let k2 = Some(r2);
    if let Some(x) = k2 { x.push('!'); }
}
fn into_scrutinee() {
    let mut s = String::from("c");
    let r3 = &mut s;
    if let Some(x) = Some(r3) {
        x.push('!');
    }
    println!("{}", s);
}
fn into_box_vec_wrapper() {
    let mut s = String::from("d");
    let mut t = String::from("e");
    let mut u = String::from("f");
    let r4 = &mut s;
    let r5 = &mut t;
    let r6 = &mut u;
    let k4 = Box::new(r4);
    let k5 = vec![r5];
    let k6 = W(r6);
    k4.push('!');
    println!("{}", k5.len());
    k6.0.push('!');
}
fn nested_and_branch(c: bool) {
    let mut s = String::from("g");
    let r7 = &mut s;
    let k7 = id(id(r7));
    k7.push('!');
    let r8 = &mut s;
    if c {
        let k8 = id(r8);
        k8.push('!');
    }
    println!("{}", s);
}
fn main() {
    through_id();
    into_some();
    into_scrutinee();
    into_box_vec_wrapper();
    nested_and_branch(true);
}
