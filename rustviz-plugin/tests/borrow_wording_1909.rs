struct Held<'a> { value: &'a String }

fn choose<'a>(left: &'a String, right: &'a String) -> &'a String {
    if left.len() > right.len() { left } else { right }
}

fn wrap(value: &String) -> Held<'_> { Held { value } }

fn ignore_shared(_: &String) {}
fn ignore_mut(_: &mut String) {}

fn main() {
    let mut a = String::from("left");
    let mut b = String::from("right");
    let r = &a;
    ignore_shared(r);
    ignore_mut(&mut a);
    a.push('!');
    let held = wrap(choose(&a, &b));
    println!("{}", held.value);
    a.clear();
    b.clear();
}
