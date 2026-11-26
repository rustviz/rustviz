// #1482: a holder assigned a conditional whose branch gives it a `&mut`
// the branch declares (`v = if c { let r = &mut a; vec![r] } else { .. }`).
// v's type is known, so rustc reborrows r there (`&mut *r`; `let v = if ..`
// moves r instead). r goes out of scope at its branch's end, and what v
// holds from then on is r's loan of a (or b), until v's last use on line 23:
// v gives a and b back there; each r goes out of scope still lent to v. main
// ended v's borrow at the join ("v's mutable borrow ends", v used on the
// next line) and r's after r was out of scope.
fn main() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut z = String::from("z");
    let c = a.len() > 0;
    let mut v = vec![&mut z];
    v[0].push('!');
    v = if c {
        let r = &mut a;
        vec![r]
    } else {
        let r = &mut b;
        vec![r]
    };
    v[0].push('?');
    println!("{} {} {}", a, b, z);
    opt(c);
    matched(a.len());
    ctor(c);
    tuple(c);
    field(c);
}

// An `Option<&mut String>`: y borrows s1 or s2, not "*r2"; x, moved out of
// y, may point into either.
fn opt(c: bool) {
    let mut s1 = String::from("a");
    let mut s2 = String::from("b");
    let mut y = None;
    y = if c {
        Some(&mut s1)
    } else {
        let r2 = &mut s2;
        Some(r2)
    };
    if let Some(x) = y {
        x.push('!');
    }
    println!("{} {}", s1, s2);
}

// A `match`.
fn matched(n: usize) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut w = vec![&mut b];
    w[0].push('!');
    w = match n {
        0 => {
            let q = &mut a;
            vec![q]
        }
        _ => vec![&mut b],
    };
    w[0].push('?');
    println!("{} {}", a, b);
}

// The conditional inside the constructor: `Some(if c { .. r } ..)`.
fn ctor(c: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut z = String::from("z");
    let mut y = Some(&mut z);
    if let Some(x) = y.as_mut() {
        x.push('!');
    }
    y = Some(if c {
        let r = &mut a;
        r
    } else {
        let r = &mut b;
        r
    });
    if let Some(x) = y {
        x.push('?');
    }
    println!("{} {} {}", a, b, z);
}

// A tuple drawn as part columns: t.0 holds the reborrow.
fn tuple(c: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut z = String::from("z");
    let mut t = (&mut z, 1);
    t.0.push('!');
    t = if c {
        let r = &mut a;
        (r, 2)
    } else {
        let r = &mut b;
        (r, 3)
    };
    t.0.push('?');
    let k = t.1;
    println!("{} {} {} {}", a, b, z, k);
}

// A struct whose field holds the borrow (`Option<&mut String>`).
struct W<'a> {
    o: Option<&'a mut String>,
}

fn field(c: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut z = String::from("z");
    let mut w = W { o: Some(&mut z) };
    if let Some(x) = w.o.as_mut() {
        x.push('!');
    }
    w = if c {
        let r = &mut a;
        W { o: Some(r) }
    } else {
        let r = &mut b;
        W { o: Some(r) }
    };
    if let Some(x) = w.o {
        x.push('?');
    }
    println!("{} {} {}", a, b, z);
    typed_let(c);
}

// `let` with the type written out: rustc reborrows r here too (main: v2's
// borrow ended at the join).
fn typed_let(c: bool) {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut v2: Vec<&mut String> = if c {
        let r = &mut a;
        vec![r]
    } else {
        let r = &mut b;
        vec![r]
    };
    v2[0].push('?');
    println!("{} {}", a, b);
}
