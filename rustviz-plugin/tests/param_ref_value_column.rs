// `ref` bindings of by-value parameters whose type needs drop (#691):
// the parameter's value gets a `(parameter)` column that the binding
// borrows and that is dropped at the end of the function.

fn two(ref a: String, ref b: Vec<i32>) -> usize {
    a.len() + b.len()
}

fn small(ref n: i32) -> i32 {
    *n
}

fn main() {
    let s = String::from("a");
    let v = vec![1];
    let n1 = two(s, v);
    let n2 = small(3);
    println!("{} {}", n1, n2);
}
