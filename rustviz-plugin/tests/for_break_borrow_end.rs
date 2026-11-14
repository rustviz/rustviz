// #872: a `for` loop over a borrow whose body can `break` (or `return`)
// still goes round again on the other paths, and `next()` then uses the
// loop's iterator, so the loan lasts to the loop's closing brace. A
// `continue` goes round again too: it doesn't end the loan.
fn main() {
    let v = vec![String::from("a"), String::from("b")];
    for a in &v {
        println!("{}", a);
        if v.len() > 1 {
            break;
        }
    }
    for b in v.iter() {
        println!("{}", b);
        if b.is_empty() {
            break;
        }
    }
    for d in &v {
        match d.len() {
            0 => break,
            1 => continue,
            _ => println!("{}", d),
        }
    }
    let mut w = vec![String::from("c")];
    for c in &mut w {
        c.push('!');
        if c.len() > 1 {
            break;
        } else {
            println!("short");
        }
    }
    println!("{} {} {}", v.len(), w.len(), first_long());
}

fn first_long() -> usize {
    let u = vec![String::from("x"), String::from("yy")];
    for s in &u {
        if s.len() > 1 {
            return s.len();
        }
    }
    u.len()
}
