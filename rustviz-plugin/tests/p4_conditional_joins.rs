#![allow(dead_code, unused_variables, unused_assignments)]
enum D { Tag(&'static str) }
impl Drop for D {
    fn drop(&mut self) {
        match self { Self::Tag(label) => println!("{label}") }
    }
}

fn one(c: bool) {
    let one = D::Tag("one");
    if c {
        drop(one);
    }
}

fn all(c: bool) {
    let all = D::Tag("all");
    match c {
        true => drop(all),
        false => drop(all),
    }
}

fn returning(c: bool) {
    let kept = D::Tag("kept");
    match c {
        true => { drop(kept); return; }
        false => (),
    }
    let _observed = &kept;
}

fn residual(c: bool, input: Result<(), ()>) -> Result<(), ()> {
    let mut value = D::Tag("old");
    if c {
        drop(value);
    } else {
        input?;
        value = D::Tag("new");
    }
    Ok(())
}

fn absent(c: bool) {
    let assigned;
    if c {
        assigned = D::Tag("assigned");
    }
}

fn replace_after_residual(c: bool, d: bool, input: Result<(), ()>) -> Result<(), ()> {
    let mut after = D::Tag("old");
    if c {
        drop(after);
        input?;
        after = D::Tag("new");
    } else if d {
        drop(after);
    }
    Ok(())
}

fn main() {
    for c in [true, false] {
        println!("one({c})");
        one(c);
        println!("all({c})");
        all(c);
        println!("returning({c})");
        returning(c);
        println!("absent({c})");
        absent(c);
    }

    println!("residual: Err");
    let _ = residual(false, Err(()));
    println!("residual: Ok");
    let _ = residual(false, Ok(()));
    println!("after move: Err");
    let _ = replace_after_residual(true, false, Err(()));
    println!("after move: Ok");
    let _ = replace_after_residual(true, false, Ok(()));
    println!("other arm moves");
    let _ = replace_after_residual(false, true, Ok(()));
    println!("other arm keeps");
    let _ = replace_after_residual(false, false, Ok(()));
}
