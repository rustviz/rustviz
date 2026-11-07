// #579 (the issue's snippet): `z = a` in the second part takes effect
// before `z > 0` is checked, so when that check fails the else branch
// runs with `z` already assigned. When `y` is `None` it runs with `z`
// still 0. So the else column starts with `z` holding either value.
// (This was refused.)

fn main() {
    let y = Some(1);
    let mut z = 0;
    if let Some(a) = y && { z = a; z > 0 } {
        println!("{}", z);
    } else {
        println!("no {}", z);
    }
}
