// #1431: a `&mut` declared in a branch whose value goes into a reference
// declared outside (`m = if c { let w = &mut s; w } else { &mut t };`) is
// reborrowed through (`&mut *w`, a coercion). m then borrows what w
// borrowed, s, until m's last use; w's own borrow ends with its scope. The
// other branch's lender t stays borrowed to m's last use too: it used to be
// given back at the join, and w's borrow ended after w had gone out of
// scope.
fn main() {
    let mut s = String::from("s");
    let mut t = String::from("t");
    let m: &mut String = if s.len() > 0 { let w = &mut s; w } else { &mut t };
    m.push('?');
    let mut u = String::from("u");
    let mut n = &mut u;
    n.push('!');
    n = if t.len() > 0 { let w2 = &mut t; &mut *w2 } else { &mut u };
    n.push('?');
    let mut o: Option<&mut String> = Some(&mut s);
    n = match o.take() { Some(x) => x, None => &mut u };
    n.push('.');
}
