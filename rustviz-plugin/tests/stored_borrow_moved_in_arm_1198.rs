// #1198: a value holding a borrow a call stored (`keepv(&mut u, &t)`)
// moved on in an arm (`let x = u;`) takes the borrow with it: the borrow
// ends on x (or y), where it is last used, not on u after u was moved.
fn keepv<'a>(hold: &mut Vec<&'a String>, r: &'a String) {
    hold.push(r);
}

fn both_arms(c: bool) {
    let t = String::from("t");
    let mut u = Vec::new();
    keepv(&mut u, &t);
    if c {
        let x = u;
        println!("{}", x.len());
    } else {
        let y = u;
        println!("{}", y.len());
    }
    println!("{}", t);
}

fn twice(c: bool) {
    let t = String::from("t");
    let mut u = Vec::new();
    keepv(&mut u, &t);
    if c {
        let x = u;
        let z = x;
        println!("{}", z.len());
    }
    println!("{}", t);
}

fn main() {
    both_arms(true);
    twice(true);
}
