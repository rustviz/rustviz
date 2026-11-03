// Assigning a field of a method call's result (#175). The temporary
// `&mut P` has no column, so the write isn't drawn; the call itself
// still shows on v. Used to panic in `resource_of_lhs`.
struct P {
    n: i32,
}

fn main() {
    let mut v = vec![P { n: 1 }];
    v.last_mut().unwrap().n = 5;
    println!("{}", v[0].n);
}
