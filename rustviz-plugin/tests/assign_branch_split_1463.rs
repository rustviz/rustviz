// #1463: a holder assigned a conditional (`q = if c { .. } else { .. };`)
// gets each branch's value in that branch, as `let q = if ..` does: the
// borrow of a lender the branches split (a is used in the first one) is
// drawn on that lender's branch leg, and q's column splits too.
#[derive(Debug)]
struct W<'x> {
    o: Option<&'x String>,
}

fn main() {
    let a = String::from("a");
    let b = String::from("b");
    let c = a.len() > 0;
    let mut q = None;
    println!("{:?}", q);
    q = if c {
        println!("{}", a);
        Some(&a)
    } else {
        Some(&b)
    };
    println!("{:?}", q);

    let mut m = None;
    println!("{:?}", m);
    m = match c {
        true => {
            let r2 = &a;
            Some(r2)
        }
        false => Some(&b),
    };
    println!("{:?}", m);

    let mut w = W { o: None };
    println!("{:?}", w);
    w = if c {
        let r3 = &a;
        W { o: Some(r3) }
    } else {
        W { o: Some(&b) }
    };
    println!("{:?}", w);
}
