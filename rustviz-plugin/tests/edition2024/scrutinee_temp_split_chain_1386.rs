// #1386 in let chains: the `let`'s scrutinee temporary forks at the `if`.
// Made by a later part (`k && let ..`), it exists on the else path only if
// the parts before got as far as its `let`.
fn make_res(n: u32) -> Result<String, String> {
    if n > 0 { Ok(String::new()) } else { Err(String::new()) }
}

fn later_part(k: bool) {
    if k && let Ok(s) = make_res(1) {
        println!("{}", s);
    } else {
        println!("no");
    }
}

fn first_part(k: bool) {
    if let Ok(s) = make_res(1) && k {
        println!("{}", s);
    } else {
        println!("no");
    }
}

fn main() {
    later_part(true);
    first_part(false);
}
