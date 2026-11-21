// #133, #278, #347: a closure's captures are drawn where it is made;
// what its body does when it runs isn't drawn where it is called. The
// closure's first dot says so. A closure that captures nothing gets no
// note, nor does an FnOnce one, whose call is drawn with what happens to
// what it captured.
fn show(_s: &String) {}

fn main() {
    let s = String::from("hi");
    let f = || show(&s);
    let g = || f();
    g();
    let o = Some(1);
    let _ = o.map(|_| f());
    let add = |a: i32| a + 1;
    let t = String::from("t");
    let eat = move || drop(t);
    eat();
    println!("{} {}", s, add(1));
}
