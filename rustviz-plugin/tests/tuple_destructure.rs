// Tuple destructuring binds (issue #86). `let (a, b) = (e1, e2);`
// should produce the same shape we'd produce for the equivalent
// two-statement form: two timeline columns with independent moves
// from each `String::from` to its binding. Pre-fix the timeline
// was empty because visit_local only handled PatKind::Binding.

fn main() {
    let (a, b) = (String::from("x"), String::from("y"));
    println!("{} {}", a, b);
}
